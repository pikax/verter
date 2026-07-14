//! Structural translation of TypeProvider completion-resolve auto-import edits.
//!
//! When the user accepts a completion that needs an import, tsserver/TSGO return the
//! auto-import as a `completionItem/resolve` `additionalTextEdit` whose `start`/`end` are
//! byte offsets into the GENERATED TSX. For a brand-new import TypeScript places that edit
//! at the top-of-file / sorted-import boundary, which in Verter's generated TSX lands inside
//! the synthetic, UNMAPPED helper-import preamble (`import { defineComponent } …`, the
//! `@verter/types` helpers, …). The strict
//! [`ProviderPositionMapper`](crate::documents::provider_projection::ProviderPositionMapper)
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
use crate::documents::provider_projection::ProviderPositionMapper;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use crate::type_provider::merge;

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
    /// anchor. Returns `None` if the anchor offset is not a valid position in `carrier_li`.
    pub fn build_edit(&self, import_texts: &[String], carrier_li: &LineIndex) -> Option<TextEdit> {
        let borrowed: Vec<&str> = import_texts.iter().map(String::as_str).collect();
        self.build_edit_borrowed(&borrowed, carrier_li)
    }

    /// Like [`Self::build_edit`] but accepts BORROWED import texts, so a caller that already holds
    /// `&str` slices (the code-action merge, which avoids cloning the provider edit's `new_text`)
    /// builds the carrier edit without owning the inputs. The owned output `new_text` is assembled
    /// here by copying the borrowed texts into one block; the inputs are never mutated.
    pub(crate) fn build_edit_borrowed(
        &self,
        import_texts: &[&str],
        carrier_li: &LineIndex,
    ) -> Option<TextEdit> {
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
                        + import_texts.iter().map(|t| t.len()).sum::<usize>(),
                );
                text.push_str(open_tag);
                for t in import_texts {
                    text.push_str(t);
                }
                text.push_str(close_tag);
                (*offset, text)
            }
        };
        let pos = carrier_li.offset_to_position(offset)?;
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
    carrier_source: &str,
    user_import_spans: &[(u32, u32)],
) -> ScriptImportInsertionAnchor {
    let blocks = scan_sfc_blocks(carrier_source);

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
            Some(abs_end) => skip_line_breaks(carrier_source, abs_end),
            // No in-block imports — insert at the content start, past a single leading line
            // break so the import is not glued to the `<script setup …>` tag.
            None => skip_one_line_break(carrier_source, content_start),
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

/// Resolve the carrier import anchor for the add-import quick-fix re-anchor — USE-SITE-AWARE and
/// fail-closed. Returns `Some` ONLY when:
/// 1. the request's carrier classifies as a **Vue SFC** via the carrier-generic
///    [`crate::server::carrier_language_for`] over the carrier stem of `current_tsx_path`, mapped by
///    the shared fail-closed [`carrier_kind_for_language`](crate::features::auto_close_tag::carrier_kind_for_language)
///    descriptor-identity classifier (mirroring `carrier_kind_for_on_type`; no banned `.is_vue()`
///    routing predicate, no `!is_svelte()` open fallback). A Svelte / non-carrier stem — and any
///    future markup carrier without its own `CarrierKind` arm — yields `None`, so the import is never
///    re-anchored into a synthesized (Vue-only) `<script setup>` block spliced onto the wrong source;
///    AND
/// 2. that Vue SFC has an **existing** `<script setup>` block — [`resolve_script_import_anchor`]
///    returns [`ScriptImportInsertionAnchor::ExistingScriptSetup`]. A Vue SFC with no `<script setup>`
///    resolves to `CreateScriptSetup`, which is DROPPED here: synthesizing a brand-new block from a
///    quick-fix would not add the import where the unresolved symbol lives (e.g. an Options-API
///    `<script>`), so it fails closed rather than mis-place;
///    AND
/// 3. that `<script setup>` is the UNAMBIGUOUS import target — the SFC does NOT also carry a non-empty
///    normal `<script>` (Options-API) block. `<script>` and `<script setup>` are SEPARATE module
///    scopes: an import added to `<script setup>` does NOT resolve a symbol whose unresolved use-site
///    is in the plain `<script>`. The re-anchor cannot prove WHICH block the use-site lives in (that
///    needs use-site/region threading, carrier-membership-adjacent and out of this resolver's scope),
///    so on the AMBIGUOUS mixed-script case it DROPS rather than guess `<script setup>` and mis-place.
///    Block composition is read from the typed [`scan_sfc_blocks`] classification the anchor resolver
///    itself uses — never a new string scanner.
///
/// This is the carrier-keyed import-reanchor capability the carrier-NEUTRAL code-action merge layer
/// ([`crate::type_provider::merge::merge_code_actions`]) depends on: the merge layer takes the
/// resulting precomputed [`ScriptImportInsertionAnchor`] and never performs a carrier classification
/// itself. The Vue-specificity is an internal detail keyed on the neutral carrier classification, so
/// no Vue gate sits in the shared merge routing.
///
/// It mirrors the discipline of the component-completion path (`build_auto_import_edit`'s
/// `ExistingScriptSetup`-only gate) and the completion-resolve self-file guard. The completion-resolve
/// path itself accepts a synthesized `CreateScriptSetup` (Volar parity) because it has ALREADY proven
/// a real Vue carrier that is not a self-file projection; the code-action merge has weaker context
/// here, so it restricts to the provable-correct anchor.
pub(crate) fn resolve_carrier_preamble_import_anchor(
    current_tsx_path: &str,
    carrier_source: &str,
    user_import_spans: &[(u32, u32)],
) -> Option<ScriptImportInsertionAnchor> {
    // The carrier stem is the IDE virtual path minus the trailing `.tsx`/`.jsx`. The branch only
    // runs for `is_carrier_ide_path(current_tsx_path)` edits, so the suffix is present; guard anyway.
    let carrier_stem = current_tsx_path
        .strip_suffix(".tsx")
        .or_else(|| current_tsx_path.strip_suffix(".jsx"))?;
    // Carrier-keyed Vue classification — carrier-generic routing (no Vue-only `.is_vue()` predicate,
    // which the carrier-routing guard bans). Mirrors `carrier_kind_for_on_type`: the carrier stem is
    // classified to a `FileLanguage` via the shared carrier classifier, then mapped to a `CarrierKind`
    // by the fail-closed, descriptor-identity `carrier_kind_for_language`. The `<script setup>`
    // import-reanchor is Vue-SFC-specific, so ONLY a `Some(CarrierKind::Vue)` continues; a Svelte /
    // non-carrier stem — and any future markup carrier without its own arm — maps to `Svelte` / `None`
    // and fails closed here, never falling through into the Vue branch.
    let carrier_continues =
        crate::server::carrier_language_for(carrier_stem).is_some_and(|language| {
            matches!(
                crate::features::auto_close_tag::carrier_kind_for_language(&language),
                Some(crate::features::auto_close_tag::CarrierKind::Vue)
            )
        });
    if !carrier_continues {
        return None;
    }

    // AMBIGUITY GATE (fail-closed): a Vue SFC may carry BOTH a normal `<script>` (Options API) AND a
    // `<script setup>`. They are separate scopes, so re-anchoring an add-import into `<script setup>`
    // when the unresolved use-site is actually in the plain `<script>` mis-places it. We cannot prove
    // the use-site block here, so when a NON-EMPTY normal `<script>` coexists with the setup block we
    // DROP rather than guess. Uses the same typed `scan_sfc_blocks` classification as the resolver
    // (no string sniffing): a normal `<script>` is `tag_name == "script" && !is_setup()`, and "non-
    // empty" is non-whitespace inner content.
    let blocks = scan_sfc_blocks(carrier_source);
    let has_nonempty_normal_script = blocks.iter().any(|b| {
        if b.tag_name != "script" || b.is_setup() {
            return false;
        }
        let (start, end) = b.content_range();
        carrier_source
            .get(start as usize..end as usize)
            .is_some_and(|inner| !inner.trim().is_empty())
    });
    if has_nonempty_normal_script {
        return None;
    }

    match resolve_script_import_anchor(carrier_source, user_import_spans) {
        anchor @ ScriptImportInsertionAnchor::ExistingScriptSetup { .. } => Some(anchor),
        // No `<script setup>` to extend — do NOT synthesize a block from a quick-fix.
        ScriptImportInsertionAnchor::CreateScriptSetup { .. } => None,
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
    /// An inserted-import edit named a carrier companion (or a bare `./Comp` resolving to a
    /// carrier) that the shared specifier-rewrite layer could NOT unambiguously map to a single
    /// user-facing `.vue`/`.svelte` form (e.g. a bare `./Comp` matching both `Comp.vue` and
    /// `Comp.svelte`). The whole resolve is rejected rather than insert a leaking companion
    /// specifier (fail-closed, the §2.9 specifier-rewrite contract).
    LeakingSpecifierDropped,
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
            AutoImportEditMappingError::LeakingSpecifierDropped => f.write_str(
                "inserted-import carrier specifier could not be unambiguously mapped to a \
                 user-facing .vue/.svelte form; resolve dropped fail-closed",
            ),
        }
    }
}

impl std::error::Error for AutoImportEditMappingError {}

/// Whether the generated-TSX `[start, end)` of an unmapped provider edit is structurally a
/// re-anchorable auto-import insertion: a ZERO-WIDTH insertion located within the synthetic
/// helper-import preamble. Proven from STRUCTURE only — the edit's geometry and the typed
/// preamble-end boundary the IDE codegen publishes on the source map
/// ([`ProviderPositionMapper::helper_preamble_end`]) — never from `new_text` content (the
/// no-text-sniffing rule).
///
/// The boundary is the generated-TSX position immediately after the last emitted helper import. An
/// insertion at or before it lands in the preamble (re-anchorable); anything past it is trailing
/// synthetic component/export code and is NOT a preamble insertion. The boundary is the
/// AUTHORITATIVE gate: it is exact even when the generated file has no mapped runs (an empty
/// `<script setup>`) and when user imports precede the helper preamble (a companion `<script>`),
/// the two cases a "before the first mapped run" heuristic gets wrong. With no boundary metadata
/// the edit cannot be proven to be in the preamble, so it is rejected — never re-anchored on a guess.
pub(crate) fn is_preamble_import_insertion(
    start: u32,
    end: u32,
    tsx_li: &LineIndex,
    mapper: &ProviderPositionMapper,
) -> bool {
    // A non-empty range is a replacement of synthetic code, not an insertion.
    if start != end {
        return false;
    }
    // Must address a real position inside the generated TSX (rejects out-of-range offsets).
    let Some(pos) = tsx_li.offset_to_position(start) else {
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

/// A single provider import edit borrowed for re-anchoring: byte offsets into the generated TSX plus
/// a BORROWED replacement text. Lets the code-action merge classify/re-anchor an edit WITHOUT
/// cloning its `new_text` — the owned text only moves into the final [`TextEdit`] when the re-anchor
/// actually succeeds (the build coalesces by copying the borrowed texts into one block).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BorrowedImportEdit<'a> {
    /// Byte offset start in the generated TSX.
    pub start: u32,
    /// Byte offset end in the generated TSX.
    pub end: u32,
    /// The replacement / inserted text (for a new import, a full `import … from '…'` line).
    pub new_text: &'a str,
}

/// The outcome of [`reanchor_preamble_import_edits`] over a set of strict-mapper-MISSED provider
/// edits. The two callers act on it differently — the completion translator turns the two failure
/// fields into structured `Err`s (all-or-nothing); the code-action merge ignores them and simply
/// drops (fail-closed) — but the CLASSIFY + ANCHOR + BUILD decision is computed in exactly one place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReanchorOutcome {
    /// The single coalesced carrier [`TextEdit`] that places every re-anchorable preamble import at
    /// `anchor`, or `None` when no input edit was a re-anchorable preamble insertion (or the anchor
    /// could not build an edit — see `anchor_missing`).
    pub reanchored: Option<TextEdit>,
    /// The `(start, end)` of the FIRST (in input order) missed edit that is NOT a preamble import
    /// insertion — a replacement of synthetic code, a zero-width edit in a non-preamble synthetic
    /// region, or an out-of-range offset. The completion translator rejects the whole resolve on it
    /// ([`AutoImportEditMappingError::UnmappableEdit`]); the code-action path drops.
    pub first_non_preamble_miss: Option<(u32, u32)>,
    /// `true` when at least one input edit WAS a re-anchorable preamble insertion but no usable
    /// anchor was supplied (or the anchor failed to build) — so the imports could not be placed.
    /// The completion translator rejects with [`AutoImportEditMappingError::NoInsertionAnchor`]; the
    /// code-action path drops.
    pub anchor_missing: bool,
}

/// THE single preamble re-anchor used by BOTH the completion-resolve translator
/// ([`translate_completion_import_edits`]) and the code-action merge
/// ([`crate::type_provider::merge::merge_code_actions`]). Given the provider edits that already
/// MISSED the strict mapper, it:
/// 1. classifies each via [`is_preamble_import_insertion`] (the typed helper-preamble-end boundary —
///    a `SelfFile` projection has no boundary ⇒ every edit is a non-preamble miss, fail-closed);
/// 2. coalesces the preamble import texts IN INPUT ORDER and builds ONE carrier [`TextEdit`] at the
///    caller-supplied `anchor` via [`ScriptImportInsertionAnchor::build_edit`] (so N imports land in
///    one block / synthesize at most one `<script setup>` — never N overlapping zero-width inserts);
/// 3. reports the first non-preamble miss and whether an anchor was needed but absent.
///
/// The `anchor` is the SINGLE policy seam: each caller resolves AND gates it for its own use-site
/// before calling. The completion path (a proven Vue carrier that is not a self-file projection)
/// passes any [`ScriptImportInsertionAnchor`], including a synthesized `CreateScriptSetup` (Volar
/// parity). The code-action path passes ONLY an `ExistingScriptSetup` of a Vue carrier and `None`
/// otherwise, so a Svelte / non-Vue / no-`<script setup>` carrier fails closed (it never synthesizes
/// a block from a quick-fix). Passing `anchor = None` with preamble imports present yields
/// `anchor_missing = true` and no edit. The caller MUST only invoke this for the CURRENT request's
/// TSX — `tsx_li` / `mapper` describe the queried file, so a foreign carrier `.tsx` edit must be
/// screened out before reaching here.
pub(crate) fn reanchor_preamble_import_edits(
    missed_edits: &[BorrowedImportEdit<'_>],
    tsx_li: &LineIndex,
    mapper: &ProviderPositionMapper,
    anchor: Option<&ScriptImportInsertionAnchor>,
    carrier_li: &LineIndex,
) -> ReanchorOutcome {
    let mut anchored_imports: Vec<&str> = Vec::new();
    let mut first_non_preamble_miss: Option<(u32, u32)> = None;

    for edit in missed_edits {
        if is_preamble_import_insertion(edit.start, edit.end, tsx_li, mapper) {
            anchored_imports.push(edit.new_text);
        } else if first_non_preamble_miss.is_none() {
            first_non_preamble_miss = Some((edit.start, edit.end));
        }
    }

    if anchored_imports.is_empty() {
        return ReanchorOutcome {
            reanchored: None,
            first_non_preamble_miss,
            anchor_missing: false,
        };
    }

    let reanchored = anchor.and_then(|a| a.build_edit_borrowed(&anchored_imports, carrier_li));
    // Preamble imports were present (we are past the empty early-return) but could not be placed:
    // no anchor supplied, or the anchor failed to build an edit.
    let anchor_missing = reanchored.is_none();
    ReanchorOutcome {
        reanchored,
        first_non_preamble_miss,
        anchor_missing,
    }
}

/// Translate a TypeProvider's completion-resolve `additionalTextEdits` (generated-TSX byte
/// offsets) into carrier-source [`TextEdit`]s, with no silent drops.
///
/// Each edit is CLASSIFIED before the strict route, symmetric to the current-file add-import
/// code-action guard ([`crate::type_provider::merge::merge_code_actions`]): a preamble import
/// insertion is diverted to the re-anchor BEFORE any strict-mapped range is accepted, because a
/// preamble insertion can strict-map to the carrier `(0,0)` file top (ABOVE `<script setup>`, an
/// invalid import location) and must never be accepted there. Two routes, plus a rejection:
/// * an edit provably a zero-width auto-import insertion in Verter's synthetic, unmapped
///   helper-import preamble — either the typed-boundary classifier ([`is_preamble_import_insertion`])
///   OR the absent-boundary zero-width case (a carrier-IDE map that publishes no
///   `x_verter_helper_preamble_end` boundary cannot prove the edit is NOT a preamble insertion) — is
///   DIVERTED to the shared re-anchor and NEVER strict-accepted at `(0,0)`. The re-anchor places it
///   at the `<script setup>` [`ScriptImportInsertionAnchor`] WHEN one is available; with NO usable
///   anchor it FAILS CLOSED ([`AutoImportEditMappingError::NoInsertionAnchor`] for a classified
///   preamble insertion, or [`AutoImportEditMappingError::UnmappableEdit`] for the absent-boundary
///   case, where no boundary means the edit cannot be proven a preamble insertion). It is all-or-
///   nothing — never spliced at `(0,0)` and never silently dropped;
/// * any OTHER edit whose generated range round-trips through the strict [`ProviderPositionMapper`]
///   targets real mapped user source (e.g. an `AddToExisting` import extending the user's own
///   import statement PAST the preamble boundary) and is applied verbatim at its mapped carrier range;
/// * any other mapper miss — a replacement of synthetic code, a zero-width edit in a
///   non-preamble synthetic region, or an out-of-range offset — yields
///   [`AutoImportEditMappingError::UnmappableEdit`] and rejects the whole resolve.
///
/// All re-anchored imports are coalesced into a single edit at the anchor (avoiding overlapping
/// zero-width inserts and synthesizing at most one `<script setup>` block). All-or-nothing: if
/// any edit must be re-anchored but no anchor is available, the whole resolve fails.
///
/// The classify → anchor → build step for the diverted/missed edits routes through the SINGLE shared
/// [`reanchor_preamble_import_edits`] (the same primitive the code-action merge calls); this
/// translator only adds the strict-mapper verbatim route on top and turns the shared outcome's
/// failure fields into structured errors.
pub fn translate_completion_import_edits(
    edits: &[ProviderImportEdit],
    anchor: Option<&ScriptImportInsertionAnchor>,
    tsx_li: &LineIndex,
    mapper: &ProviderPositionMapper,
    carrier_li: &LineIndex,
    edit_target_path: &str,
    carrier_source_exists: &dyn Fn(&str) -> bool,
) -> Result<Vec<TextEdit>, AutoImportEditMappingError> {
    // Rewrite any carrier-COMPANION (or bare-`./Comp`) import specifier in the
    // inserted text back to the bare `.vue`/`.svelte` specifier BEFORE
    // mapping/anchoring, through the SHARED specifier-rewrite layer. The TS engine
    // can emit `from "./Comp.vue.tsx"` / `"./Comp.vue.verter.ts"` / a bare `./Comp`
    // for a bare carrier import resolved through in-project redirection; on the
    // verter_lsp LSP surface the engine returns raw responses, so this rewrite is
    // owned here. Done once up front so both the verbatim and re-anchor paths use
    // the bare specifier. FAIL CLOSED: an unmappable carrier specifier (e.g. an
    // ambiguous bare `./Comp`) rejects the WHOLE resolve rather than insert a leak.
    let edits: Vec<ProviderImportEdit> = edits
        .iter()
        .map(|e| {
            let ctx = crate::type_provider::specifier_rewrite::SpecifierRewriteCtx {
                edit_target_path,
                carrier_source_exists,
            };
            let new_text =
                match crate::type_provider::specifier_rewrite::rewrite_inserted_carrier_specifier(
                    &e.new_text,
                    &ctx,
                ) {
                    crate::type_provider::specifier_rewrite::SpecifierRewrite::Unchanged => {
                        e.new_text.clone()
                    }
                    crate::type_provider::specifier_rewrite::SpecifierRewrite::Rewritten(t) => t,
                    crate::type_provider::specifier_rewrite::SpecifierRewrite::Drop => {
                        return Err(AutoImportEditMappingError::LeakingSpecifierDropped);
                    }
                };
            Ok(ProviderImportEdit {
                start: e.start,
                end: e.end,
                new_text,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let edits = &edits;

    let mut result: Vec<TextEdit> = Vec::new();
    let mut missed: Vec<BorrowedImportEdit<'_>> = Vec::new();

    for edit in edits {
        // CLASSIFY BEFORE STRICT-ACCEPT (symmetric to the current-file code-action guard in
        // `merge::feature_merges::merge_code_actions`): a preamble import insertion can STRICT-MAP to
        // the carrier `(0,0)` file top (ABOVE `<script setup>`, an invalid import location), so it
        // must NEVER be strict-accepted. Route it to the shared re-anchor, which places it at the
        // `<script setup>` anchor or fails closed via the all-or-nothing `NoInsertionAnchor` /
        // `UnmappableEdit` path. Two structural discriminators, both STRUCTURE only (geometry + the
        // typed `x_verter_helper_preamble_end` boundary), never `new_text` and never the `(0,0)`
        // value: (1) the with-boundary classifier; (2) the absent-boundary zero-width fuse (no
        // boundary ⇒ cannot prove the edit is NOT a preamble insertion, and a real Verter carrier-IDE
        // projection always publishes the boundary).
        if is_preamble_import_insertion(edit.start, edit.end, tsx_li, mapper)
            || (edit.start == edit.end && mapper.helper_preamble_end().is_none())
        {
            missed.push(BorrowedImportEdit {
                start: edit.start,
                end: edit.end,
                new_text: &edit.new_text,
            });
            continue;
        }
        match merge::tsx_range_to_carrier_range(edit.start, edit.end, tsx_li, mapper, carrier_li) {
            // Round-trips through the strict mapper ⇒ targets real mapped user source; apply
            // verbatim at its mapped carrier range (the mapper is never bypassed for these). A
            // genuine `AddToExisting` edit extends the user's own import run PAST the preamble
            // boundary, so it is NOT diverted above and takes this verbatim route.
            Some(range) => result.push(TextEdit {
                range,
                new_text: edit.new_text.clone(),
            }),
            // Defer every strict-mapper miss to the shared re-anchor, which classifies it as a
            // preamble import insertion (re-anchorable) or a non-preamble miss (rejected).
            None => missed.push(BorrowedImportEdit {
                start: edit.start,
                end: edit.end,
                new_text: &edit.new_text,
            }),
        }
    }

    let outcome = reanchor_preamble_import_edits(&missed, tsx_li, mapper, anchor, carrier_li);
    // A non-preamble miss rejects the whole resolve (UnmappableEdit takes precedence, exactly as the
    // in-order loop did) — never a partial edit set spliced into user source.
    if let Some((start, end)) = outcome.first_non_preamble_miss {
        return Err(AutoImportEditMappingError::UnmappableEdit { start, end });
    }
    // A re-anchorable preamble import with no usable anchor rejects the whole resolve.
    if outcome.anchor_missing {
        return Err(AutoImportEditMappingError::NoInsertionAnchor);
    }
    if let Some(edit) = outcome.reanchored {
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

// The inserted-import carrier-specifier rewrite lives in the SHARED
// `crate::type_provider::specifier_rewrite` module (companion + bare-carrier
// resolution + the fail-closed `Drop`), consumed by `translate_completion_import_edits`
// above and the code-action merge. There is no second rewrite here.
