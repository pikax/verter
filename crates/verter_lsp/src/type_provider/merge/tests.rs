//! Behavior-preserving unit tests for the `type_provider::merge` module.
//!
//! `use super::*` pulls in every merge function/type re-exported from the
//! `merge` module root; the remaining imports bring in the protocol DTOs,
//! line-index/position-mapper helpers, and sourcemap builder the fixtures use.

#![allow(clippy::too_many_arguments)]

use std::sync::Arc;

use tower_lsp_server::ls_types::*;
use verter_span::TsPosition;

use super::definition::{is_carrier_ide_path, normalize_carrier_path, path_to_uri};
use super::hover::{extract_hover_text, replace_kind_prefix, strip_leading_code_block};
use super::*;
use crate::documents::line_index::LineIndex;
use crate::documents::position_map::PositionMapper;
use crate::documents::provider_projection::ProviderPositionMapper;
use crate::features::hover::HoverSourceToken;
use crate::type_provider::protocol::{
    self, Completion, CompletionKind, CompletionResolveData, CompletionResult, HoverInfo,
    RenameLocation, TypeCodeAction, TypeDiagnostic, TypeDiagnosticSeverity, TypeDocumentHighlight,
    TypeDocumentHighlightKind, TypeLocation,
};

// ── Position mapping tests ─────────────────────────────────────

fn make_mapper_and_indexes() -> (ProviderPositionMapper, LineIndex, LineIndex) {
    // Vue source (line 0-1: template, line 3-4: script)
    let carrier_source = "<template>\n  <div>{{ msg }}</div>\n</template>\n\n<script setup>\nconst msg = \"hello\";\n</script>";
    // TSX source (script at line 0)
    let tsx_source = "const msg = \"hello\";\n";

    // Source map: TSX line 0 col 0 → Vue line 5 col 0
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content("App.vue", carrier_source);
    builder.add_token(0, 0, 5, 0, Some(source_id), None);
    builder.add_token(0, 6, 5, 6, Some(source_id), None);
    builder.add_token(0, 10, 5, 10, Some(source_id), None);
    let json = builder.into_sourcemap().to_json_string();

    let mapper = ProviderPositionMapper::source_map(PositionMapper::from_json(&json).unwrap());
    let carrier_li = LineIndex::new_utf16(carrier_source);
    let tsx_li = LineIndex::new_utf16(tsx_source);

    (mapper, carrier_li, tsx_li)
}

/// @ai-generated — Vue position maps to correct TSX byte offset
#[test]
fn vue_position_maps_to_tsx_offset() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // Vue line 5, col 6 ("msg") → TSX line 0, col 6 → byte offset 6
    let offset = carrier_position_to_tsx_offset(
        &Position {
            line: 5,
            character: 6,
        },
        &carrier_li,
        &mapper,
        &tsx_li,
    );
    assert_eq!(offset, Some(6));
}

/// @ai-generated — Unmappable Vue position returns None
#[test]
fn unmappable_vue_position_returns_none() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // Line 0 is in the template, not mapped in our source map
    let offset = carrier_position_to_tsx_offset(
        &Position {
            line: 0,
            character: 0,
        },
        &carrier_li,
        &mapper,
        &tsx_li,
    );
    assert!(offset.is_none());
}

/// Range endpoint compatibility: build a mapper with TWO mapped runs separated by
/// synthetic/unmapped content. A TSX range whose endpoints fall in the two DIFFERENT
/// runs must be DROPPED by `tsx_range_to_carrier_range` (the strict run-compatibility
/// check), while a range fully inside ONE run maps correctly.
///
/// Discriminating: a per-endpoint composer maps each endpoint independently and returns
/// `Some` whenever both endpoints individually map — so the cross-run range produces a
/// bogus Vue range straddling the synthetic content. The strict API returns `None`.
#[test]
fn tsx_range_rejects_cross_run_endpoints_with_synthetic_between() {
    // TSX single line "abcXXXXdef" (byte offset == UTF-16 col).
    let tsx_source = "abcXXXXdef";
    // Vue single line, long enough to hold the mapped source columns.
    let carrier_source = &" ".repeat(80);

    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content("App.vue", carrier_source);
    // mapped run A: gen(0,0)->src(0,0), bounded to [0,3) by the unmapped token at 3.
    builder.add_token(0, 0, 0, 0, Some(source_id), None);
    // unmapped synthetic token at gen col 3 ("XXXX").
    builder.add_token(0, 3, 0, 0, None, None);
    // mapped run B: gen(0,7)->src(0,50).
    builder.add_token(0, 7, 0, 50, Some(source_id), None);
    let json = builder.into_sourcemap().to_json_string();

    let pm = PositionMapper::from_json(&json).unwrap();
    let tsx_li = LineIndex::new_utf16(tsx_source);
    let carrier_li = LineIndex::new_utf16(carrier_source);

    // Precondition: both endpoints individually map (start byte 1 -> run A,
    // end byte 9 -> run B), so the *old* per-endpoint composer returned Some.
    assert!(pm.tsx_to_carrier(TsPosition::new(0, 1)).is_some());
    assert!(pm.tsx_to_carrier(TsPosition::new(0, 9)).is_some());
    let mapper = ProviderPositionMapper::source_map(pm);

    // Cross-run range straddling the synthetic "XXXX" -> dropped.
    assert!(
        tsx_range_to_carrier_range(1, 9, &tsx_li, &mapper, &carrier_li).is_none(),
        "a TSX range whose endpoints land in two runs separated by synthetic content \
             must be dropped, not composed into a bogus Vue range"
    );

    // In-run range fully inside run A [0,3) -> maps.
    let r = tsx_range_to_carrier_range(1, 3, &tsx_li, &mapper, &carrier_li)
        .expect("range fully inside one mapped run must map");
    assert_eq!(
        r.start,
        Position {
            line: 0,
            character: 1
        }
    );
    assert_eq!(
        r.end,
        Position {
            line: 0,
            character: 3
        }
    );
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
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = make_verter_hover("**msg** (SetupConst)");
    let type_hover = HoverInfo {
        contents: "const msg: string".to_string(),
        range_start: None,
        range_end: None,
    };

    let result = merge_hover(
        Some(verter),
        Some(type_hover),
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        None,
    );
    let text = extract_hover_text(&result.unwrap());
    assert!(text.contains("const msg: string"));
    assert!(text.contains("SetupConst"));
}

/// @ai-generated — Only verter hover present
#[test]
fn merge_hover_verter_only() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = make_verter_hover("**msg** (SetupConst)");

    let result = merge_hover(
        Some(verter),
        None,
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        None,
    );
    assert!(result.is_some());
    let text = extract_hover_text(&result.unwrap());
    assert!(text.contains("SetupConst"));
}

/// @ai-generated — Only type hover present
#[test]
fn merge_hover_type_only() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let type_hover = HoverInfo {
        contents: "const msg: string".to_string(),
        range_start: None,
        range_end: None,
    };

    let result = merge_hover(
        None,
        Some(type_hover),
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        None,
    );
    assert!(result.is_some());
    let text = extract_hover_text(&result.unwrap());
    assert!(text.contains("const msg: string"));
}

/// @ai-generated — Neither hover present returns None
#[test]
fn merge_hover_neither() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let result = merge_hover(None, None, &mapper, &tsx_li, &carrier_li, None, None);
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
        text_edit_new_text: None,
        insert_text: None,
        sort_text: None,
        insert_text_format: None,
        commit_characters: None,
        filter_text: None,
        preselect: None,
        label_details: None,
        data: None,
    }
}

/// A resolve-bearing provider completion (one carrying a `CompletionResolveData`
/// handle) is tagged with the provider-NEUTRAL `verter_resolve` envelope —
/// kind + active provider id + carrier path + serialized typed resolve key —
/// and the old provider-baked `tsgo` / `original_data` keys are GONE.
///
/// Discriminating: the pre-fix `merge_completions` emitted
/// `{ "tsgo": true, "original_data": …, "tsx_path": … }`. This asserts the
/// neutral envelope shape and the ABSENCE of those keys, so it fails on the
/// old emission and passes on the new one. It also proves the envelope is
/// namespaced separately from the workspace-component `auto_import` shape.
#[test]
fn merge_completions_emits_neutral_resolve_envelope() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let mut entry = make_type_completion("computed");
    entry.data = Some(CompletionResolveData::TsserverEntry {
        name: "computed".to_string(),
        source: Some("vue".to_string()),
        data: None,
        offset: 7,
    });
    let type_result = CompletionResult {
        items: vec![entry],
        is_incomplete: false,
    };

    let (items, _) = merge_completions(
        Vec::new(),
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        Some("/workspace/App.vue.tsx"),
        "tsserver",
        false,
    );

    let item = items
        .iter()
        .find(|i| i.label == "computed")
        .expect("the resolve-bearing item survives the merge");
    let data = item
        .data
        .as_ref()
        .expect("resolve-bearing item carries data");
    let envelope = data
        .get("verter_resolve")
        .expect("the neutral verter_resolve envelope is present");
    assert_eq!(
        envelope.get("kind").and_then(|v| v.as_str()),
        Some("type_provider")
    );
    assert_eq!(
        envelope.get("provider_id").and_then(|v| v.as_str()),
        Some("tsserver"),
        "the active provider id is stamped onto the envelope"
    );
    assert_eq!(
        envelope.get("provider_path").and_then(|v| v.as_str()),
        Some("/workspace/App.vue.tsx")
    );
    // The serialized typed resolve key round-trips back to the entry.
    let provider_data = envelope
        .get("provider_data")
        .cloned()
        .expect("provider_data carries the serialized resolve key");
    let parsed: CompletionResolveData =
        serde_json::from_value(provider_data).expect("provider_data is a valid resolve key");
    assert!(matches!(
        parsed,
        CompletionResolveData::TsserverEntry { ref name, .. } if name == "computed"
    ));
    // Negative assertions: the provider-baked keys must be DELETED.
    assert!(
        data.get("tsgo").is_none(),
        "the provider-baked `tsgo` marker must be removed"
    );
    assert!(
        data.get("original_data").is_none(),
        "`original_data` must be removed (replaced by the typed provider_data)"
    );
}

/// F3: a LOCAL completion (a `TsserverEntry` with no `source`/`data`) carries
/// NO `verter_resolve` envelope — it resolves to nothing actionable, so
/// stamping every local item is per-keystroke payload bloat and a no-op
/// resolve round-trip.
///
/// Discriminating: the pre-fix `merge_completions` stamped the envelope for
/// ANY item with a `data` handle (every tsserver entry carries one). This
/// asserts the local item's `data` is `None` (no envelope) while an
/// auto-import item in the SAME list keeps its envelope — so it fails on the
/// blanket-stamp behavior and passes on actionable-only stamping.
#[test]
fn merge_completions_omits_envelope_for_nonactionable_local_item() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    // A local member: name-only handle, no source/data → not actionable.
    let mut local = make_type_completion("myLocalVar");
    local.data = Some(CompletionResolveData::TsserverEntry {
        name: "myLocalVar".to_string(),
        source: None,
        data: None,
        offset: 7,
    });
    // An auto-import: source present → actionable.
    let mut auto_import = make_type_completion("computed");
    auto_import.data = Some(CompletionResolveData::TsserverEntry {
        name: "computed".to_string(),
        source: Some("vue".to_string()),
        data: None,
        offset: 7,
    });
    let type_result = CompletionResult {
        items: vec![local, auto_import],
        is_incomplete: false,
    };

    let (items, _) = merge_completions(
        Vec::new(),
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        Some("/workspace/App.vue.tsx"),
        "tsserver",
        false,
    );

    let local_item = items
        .iter()
        .find(|i| i.label == "myLocalVar")
        .expect("the local item survives the merge");
    assert!(
        local_item.data.is_none(),
        "a non-actionable local completion must NOT carry a resolve envelope — \
             minting one is per-keystroke payload bloat and a no-op resolve"
    );

    let auto_item = items
        .iter()
        .find(|i| i.label == "computed")
        .expect("the auto-import item survives the merge");
    assert!(
        auto_item
            .data
            .as_ref()
            .and_then(|d| d.get("verter_resolve"))
            .is_some(),
        "an actionable auto-import completion KEEPS its resolve envelope"
    );
}

/// F2: a label collision between a non-resolvable retained item and an
/// incoming provider item that carries the ACTIONABLE auto-import handle must
/// move the handle onto the retained item — never silently drop it.
///
/// Discriminating: the pre-fix dedupe only upgraded `kind` on a collision and
/// dropped the incoming item wholesale, so the auto-import handle was lost
/// whenever a same-label local/plain item was already present. This asserts
/// the retained `computed` ends up carrying the envelope, which fails on the
/// old kind-only dedupe.
#[test]
fn merge_completions_dedupe_preserves_import_capable_handle() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    // A verter (or earlier) item with the same label but NO resolve handle.
    let plain = CompletionItem {
        label: "computed".to_string(),
        ..Default::default()
    };
    // The provider's auto-import entry for the same label, carrying the handle.
    let mut auto_import = make_type_completion("computed");
    auto_import.data = Some(CompletionResolveData::TsserverEntry {
        name: "computed".to_string(),
        source: Some("vue".to_string()),
        data: Some(serde_json::json!({ "exportName": "computed" })),
        offset: 7,
    });
    let type_result = CompletionResult {
        items: vec![auto_import],
        is_incomplete: false,
    };

    let (items, _) = merge_completions(
        vec![plain],
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        Some("/workspace/App.vue.tsx"),
        "tsserver",
        false,
    );

    // Only one `computed` survives (deduped), and it carries the auto-import
    // envelope adopted from the colliding provider item.
    let computed: Vec<_> = items.iter().filter(|i| i.label == "computed").collect();
    assert_eq!(computed.len(), 1, "the label is deduped to one item");
    let envelope = computed[0]
        .data
        .as_ref()
        .and_then(|d| d.get("verter_resolve"))
        .expect("the retained item ADOPTS the import-capable resolve handle");
    assert_eq!(
        envelope.get("provider_id").and_then(|v| v.as_str()),
        Some("tsserver")
    );
}

/// F2 (second case): two same-name external completions from DIFFERENT
/// `source` modules. The first carries the actionable handle and is retained;
/// the second collides. The retained item keeps an actionable handle (it does
/// not get clobbered into a non-resolvable state).
#[test]
fn merge_completions_dedupe_keeps_actionable_on_same_label_distinct_sources() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let mut from_vue = make_type_completion("ref");
    from_vue.data = Some(CompletionResolveData::TsserverEntry {
        name: "ref".to_string(),
        source: Some("vue".to_string()),
        data: None,
        offset: 7,
    });
    let mut from_other = make_type_completion("ref");
    from_other.data = Some(CompletionResolveData::TsserverEntry {
        name: "ref".to_string(),
        source: Some("@my/lib".to_string()),
        data: None,
        offset: 7,
    });
    let type_result = CompletionResult {
        items: vec![from_vue, from_other],
        is_incomplete: false,
    };

    let (items, _) = merge_completions(
        Vec::new(),
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        Some("/workspace/App.vue.tsx"),
        "tsserver",
        false,
    );

    let refs: Vec<_> = items.iter().filter(|i| i.label == "ref").collect();
    assert_eq!(refs.len(), 1, "the label is deduped to one item");
    assert!(
        refs[0]
            .data
            .as_ref()
            .and_then(|d| d.get("verter_resolve"))
            .is_some(),
        "the retained item keeps an actionable resolve handle"
    );
}

/// F1: the shared `provider_completion_to_lsp_item` helper (used by BOTH the
/// merge path and the virtual-file completion path) preserves an actionable
/// resolve handle as a `verter_resolve` envelope. This is the unit that the
/// virtual-file path now routes through, so a provider auto-import returned on
/// that path can resolve into an import edit.
///
/// Discriminating: the pre-fix virtual-file path built a `CompletionItem`
/// without the `data` field, dropping the handle entirely. This asserts the
/// helper carries it through, which the old inline mapping did not do.
#[test]
fn provider_completion_to_lsp_item_preserves_actionable_handle() {
    let mut entry = make_type_completion("computed");
    entry.data = Some(CompletionResolveData::TsserverEntry {
        name: "computed".to_string(),
        source: Some("vue".to_string()),
        data: None,
        offset: 7,
    });
    let item = provider_completion_to_lsp_item(
        entry,
        "computed".to_string(),
        None,
        "tsserver",
        Some("/ws/App.vue.tsx"),
    );
    let envelope = item
        .data
        .as_ref()
        .and_then(|d| d.get("verter_resolve"))
        .expect("the virtual-file mapper preserves the actionable resolve handle");
    assert_eq!(
        envelope.get("provider_path").and_then(|v| v.as_str()),
        Some("/ws/App.vue.tsx"),
        "the resolve re-issues against the queried generated-TSX path"
    );

    // A local (non-actionable) item carries no envelope through the same helper.
    let mut local = make_type_completion("x");
    local.data = Some(CompletionResolveData::TsserverEntry {
        name: "x".to_string(),
        source: None,
        data: None,
        offset: 0,
    });
    let local_item = provider_completion_to_lsp_item(
        local,
        "x".to_string(),
        None,
        "tsserver",
        Some("/ws/App.vue.tsx"),
    );
    assert!(
        local_item.data.is_none(),
        "a local item carries no envelope through the shared mapper either"
    );
}

/// When a provider completion's replace-range was dropped fail-closed (so
/// `text_edit` is `None`), the shared mapper carries the provider's intended
/// insert text onto the LSP item. Accepting the item then inserts that text, not
/// the display `label` (which can differ — e.g. a decorated auto-import label).
///
/// Discriminating: a mapper that left `insert_text` unset would make the client
/// fall back to the `label`, inserting `"foo (auto-import)"` instead of `"foo"`.
#[test]
fn provider_completion_to_lsp_item_carries_insert_text_when_range_dropped() {
    let mut entry = make_type_completion("foo (auto-import)");
    entry.insert_text = Some("foo".to_string());
    // Dropped range ⇒ no `text_edit`; the client commits `insert_text`.
    let item = provider_completion_to_lsp_item(
        entry,
        "foo (auto-import)".to_string(),
        None,
        "tsgo",
        Some("/ws/App.vue.tsx"),
    );
    assert_eq!(
        item.insert_text.as_deref(),
        Some("foo"),
        "the dropped-range item must commit the provider's intended insert text"
    );
    assert_ne!(
        item.insert_text.as_deref(),
        Some("foo (auto-import)"),
        "the committed text must not fall back to the decorated display label"
    );
}

/// On a dropped range, the plain-insert fallback prefers the dropped edit's
/// `textEdit.newText` over an explicit `insertText`, and never the label.
///
/// Discriminating: an item carrying `text_edit_new_text = "foo"` with a decorated
/// label and a dropped range must insert `"foo"` (the newText the dropped edit
/// would have applied), not the label.
#[test]
fn provider_completion_to_lsp_item_prefers_new_text_over_label_when_range_dropped() {
    let mut entry = make_type_completion("foo (auto-import)");
    entry.text_edit_new_text = Some("foo".to_string());
    // Dropped range ⇒ no `text_edit`; the client commits the plain-insert text.
    let item = provider_completion_to_lsp_item(
        entry,
        "foo (auto-import)".to_string(),
        None,
        "tsgo",
        Some("/ws/App.vue.tsx"),
    );
    assert_eq!(
        item.insert_text.as_deref(),
        Some("foo"),
        "the dropped-range item commits the dropped edit's newText"
    );
    assert_ne!(
        item.insert_text.as_deref(),
        Some("foo (auto-import)"),
        "the committed text must not fall back to the decorated display label"
    );
}

/// The emit wiring for the additive carrier completion fields: every field a
/// `.ts` file relies on (snippet `insert_text_format`, `commit_characters`,
/// `filter_text`, `preselect`, `label_details`) must be propagated from the
/// provider-neutral [`protocol::Completion`] carrier onto the LSP
/// [`CompletionItem`] by [`provider_completion_to_lsp_item`].
///
/// Discriminating: pre-fix the carrier had NO such fields (and the mapper
/// emitted only `label`/`kind`/`detail`/`documentation`/`sort_text`/
/// `insert_text`/`text_edit`/`data`, defaulting the rest to `None`), so this
/// test does not compile against the unmodified tree, and once the fields exist
/// it FAILS unless the mapper actually copies each one through.
#[test]
fn provider_completion_to_lsp_item_propagates_carrier_fields() {
    let mut entry = make_type_completion("createApp");
    entry.insert_text_format = Some(protocol::CompletionInsertTextFormat::Snippet);
    entry.commit_characters = Some(vec!["(".to_string(), ".".to_string()]);
    entry.filter_text = Some("createApp".to_string());
    entry.preselect = Some(true);
    entry.label_details = Some(protocol::CompletionLabelDetails {
        detail: Some("(app: App)".to_string()),
        description: Some("vue".to_string()),
    });

    let item = provider_completion_to_lsp_item(
        entry,
        "createApp".to_string(),
        None,
        "tsgo",
        Some("/ws/App.vue.tsx"),
    );

    assert_eq!(
        item.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "a snippet carrier must surface as InsertTextFormat::SNIPPET on the LSP item"
    );
    assert_eq!(
        item.commit_characters,
        Some(vec!["(".to_string(), ".".to_string()]),
        "commit_characters must propagate verbatim"
    );
    assert_eq!(
        item.filter_text.as_deref(),
        Some("createApp"),
        "filter_text must propagate"
    );
    assert_eq!(item.preselect, Some(true), "preselect must propagate");
    let label_details = item
        .label_details
        .expect("label_details must propagate onto the LSP item");
    assert_eq!(label_details.detail.as_deref(), Some("(app: App)"));
    assert_eq!(label_details.description.as_deref(), Some("vue"));
}

/// Fail-closed negative: a carrier with NONE of the new fields set must NOT
/// fabricate them on the LSP item — every new field stays `None`. This pins the
/// "parse what the wire carries; leave None otherwise; NEVER fabricate" rule at
/// the emit boundary.
#[test]
fn provider_completion_to_lsp_item_does_not_fabricate_carrier_fields() {
    // `make_type_completion` leaves every new field `None`.
    let entry = make_type_completion("plainMember");
    let item = provider_completion_to_lsp_item(
        entry,
        "plainMember".to_string(),
        None,
        "tsgo",
        Some("/ws/App.vue.tsx"),
    );
    assert_eq!(
        item.insert_text_format, None,
        "a non-snippet carrier must NOT fabricate an insert_text_format"
    );
    assert_ne!(
        item.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "a plain carrier is never a snippet"
    );
    assert_eq!(item.commit_characters, None);
    assert_eq!(item.filter_text, None);
    assert_eq!(item.preselect, None);
    assert!(item.label_details.is_none());
}

/// @ai-generated — TypeProvider completions are added alongside verter completions
#[test]
fn merge_completions_combines_both() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = vec![make_verter_completion("msg")];
    let type_result = CompletionResult {
        items: vec![make_type_completion("count"), make_type_completion("name")],
        is_incomplete: false,
    };

    let (result, is_incomplete) = merge_completions(
        verter,
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        "tsgo",
        false,
    );
    assert_eq!(result.len(), 3);
    assert!(!is_incomplete);
    let labels: Vec<&str> = result.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"msg"));
    assert!(labels.contains(&"count"));
    assert!(labels.contains(&"name"));
}

/// A surviving completion replace-range commits the provider's `textEdit.newText`
/// — NOT an explicit `insertText` — when the two differ. Per LSP, when a
/// completion item carries a `textEdit`, the editor applies `textEdit.newText`
/// and ignores `insertText`; so the surviving edit's text must be the newText.
///
/// Discriminating: the pre-fix surviving-edit branch built the edit's `new_text`
/// from `insert_text`, so an item with `insert_text = "WRONG"` and
/// `text_edit_new_text = "foo"` produced an edit that committed `"WRONG"`. This
/// asserts the surviving edit commits `"foo"` (the newText).
#[test]
fn merge_completions_surviving_edit_commits_text_edit_new_text_not_insert_text() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let mut entry = make_type_completion("foo");
    // A surviving TSX replace-range (line 0, cols 0..10 all map to the carrier).
    entry.edit_range_start = Some(0);
    entry.edit_range_end = Some(10);
    // The two text fields DIFFER: the edit must use the newText, not insertText.
    entry.text_edit_new_text = Some("foo".to_string());
    entry.insert_text = Some("WRONG".to_string());
    let type_result = CompletionResult {
        items: vec![entry],
        is_incomplete: false,
    };

    let (result, _) = merge_completions(
        vec![],
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        "tsgo",
        false,
    );
    let item = result.iter().find(|i| i.label == "foo").expect("foo item");
    let new_text = match item.text_edit.as_ref().expect("a surviving text edit") {
        CompletionTextEdit::Edit(edit) => edit.new_text.as_str(),
        CompletionTextEdit::InsertAndReplace(edit) => edit.new_text.as_str(),
    };
    assert_eq!(
        new_text, "foo",
        "the surviving edit must commit the provider's textEdit.newText"
    );
    assert_ne!(
        new_text, "WRONG",
        "the surviving edit must NOT commit an explicit insertText when it differs from newText"
    );
}

/// @ai-generated — Duplicate labels are deduplicated (verter wins)
#[test]
fn merge_completions_deduplicates() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = vec![make_verter_completion("msg")];
    let type_result = CompletionResult {
        items: vec![make_type_completion("msg")], // duplicate
        is_incomplete: false,
    };

    let (result, _) = merge_completions(
        verter,
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        "tsgo",
        false,
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].label, "msg");
}

/// @ai-generated — ___VERTER___ prefixed completions are filtered
#[test]
fn merge_completions_filters_verter_internal() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = vec![];
    let type_result = CompletionResult {
        items: vec![
            make_type_completion("msg"),
            make_type_completion("___VERTER___hidden"),
        ],
        is_incomplete: false,
    };

    let (result, _) = merge_completions(
        verter,
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        "tsgo",
        false,
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].label, "msg");
}

/// @ai-generated — is_incomplete flag is propagated from TypeProvider result
#[test]
fn merge_completions_propagates_is_incomplete() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = vec![make_verter_completion("msg")];
    let type_result = CompletionResult {
        items: vec![make_type_completion("count")],
        is_incomplete: true,
    };

    let (result, is_incomplete) = merge_completions(
        verter,
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        "tsgo",
        false,
    );
    assert_eq!(result.len(), 2);
    assert!(
        is_incomplete,
        "is_incomplete should be propagated from TSGO"
    );
}

/// @ai-generated — $V_ prefixed type helpers are filtered
#[test]
fn merge_completions_filters_dollar_v_prefix() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = vec![];
    let type_result = CompletionResult {
        items: vec![
            make_type_completion("msg"),
            make_type_completion("$V_EmitsToProps"),
        ],
        is_incomplete: false,
    };

    let (result, _) = merge_completions(
        verter,
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        "tsgo",
        false,
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].label, "msg");
}

/// @ai-generated — TSGO-internal duplicates are deduplicated
#[test]
fn merge_completions_deduplicates_tsgo_internal() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = vec![make_verter_completion("msg")];
    let type_result = CompletionResult {
        items: vec![
            make_type_completion("onMounted"), // local binding
            make_type_completion("onMounted"), // auto-import suggestion (same label)
        ],
        is_incomplete: false,
    };

    let (result, _) = merge_completions(
        verter,
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        "tsgo",
        false,
    );
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
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = vec![make_verter_completion("onMounted")];
    let type_result = CompletionResult {
        items: vec![
            make_type_completion("onMounted"), // TSGO local
            make_type_completion("onMounted"), // TSGO auto-import
            make_type_completion("ref"),       // unique
        ],
        is_incomplete: false,
    };

    let (result, _) = merge_completions(
        verter,
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        "tsgo",
        false,
    );
    let on_mounted_count = result.iter().filter(|i| i.label == "onMounted").count();
    assert_eq!(
        on_mounted_count, 1,
        "onMounted should appear exactly once (from verter)"
    );
    assert_eq!(result.len(), 2); // onMounted + ref
}

/// Internal compiler identifiers like __props, __emit should be filtered
#[test]
fn merge_completions_filters_dunder_internal() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = vec![];
    let type_result = CompletionResult {
        items: vec![
            make_type_completion("msg"),
            make_type_completion("__props"),
            make_type_completion("__emit"),
            make_type_completion("__slots"),
            make_type_completion("__expose"),
        ],
        is_incomplete: false,
    };

    let (result, _) = merge_completions(
        verter,
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        "tsgo",
        false,
    );
    let labels: Vec<&str> = result.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(
        labels,
        vec!["msg"],
        "should filter __props, __emit, __slots, __expose"
    );
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
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = vec![make_verter_diagnostic("parse error")];
    let types = vec![TypeDiagnostic {
        message: "Type 'number' is not assignable to type 'string'".to_string(),
        severity: TypeDiagnosticSeverity::Error,
        start: 6, // TSX offset for "msg"
        end: 9,
        code: Some("2322".to_string()),
        tags: Vec::new(),
        related_information: Vec::new(),
    }];

    let result = merge_diagnostics(
        verter,
        types,
        "App.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].source.as_deref(), Some("verter"));
    assert_eq!(result[1].source.as_deref(), Some("ts"));
    assert!(result[1].message.contains("not assignable"));
    // A plain type error carries no editor tags.
    assert_eq!(
        result[1].tags, None,
        "an untagged type diagnostic yields tags == None"
    );
}

/// An `Unnecessary`/`Deprecated` carrier tag survives onto the published LSP
/// `Diagnostic.tags` — this is what fades an unused `<script setup>` import in
/// a `.vue` (TypeProvider is the sole diagnostic source there). Mirrors the
/// native lint-bridge tag mapping in `diagnostics_bridge.rs`.
#[test]
fn merge_diagnostics_propagates_unnecessary_and_deprecated_tags() {
    use crate::type_provider::protocol::TypeDiagnosticTag;

    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let types = vec![
        TypeDiagnostic {
            message: "'msg' is declared but its value is never read.".to_string(),
            severity: TypeDiagnosticSeverity::Hint,
            start: 6, // TSX offset for "msg"
            end: 9,
            code: Some("6133".to_string()),
            tags: vec![TypeDiagnosticTag::Unnecessary],
            related_information: Vec::new(),
        },
        TypeDiagnostic {
            message: "'msg' is deprecated.".to_string(),
            severity: TypeDiagnosticSeverity::Hint,
            start: 6,
            end: 9,
            code: Some("6385".to_string()),
            tags: vec![TypeDiagnosticTag::Deprecated],
            related_information: Vec::new(),
        },
    ];

    let result = merge_diagnostics(
        vec![],
        types,
        "App.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );
    assert_eq!(
        result.len(),
        2,
        "both diagnostics map and survive: {result:?}"
    );
    assert_eq!(
        result[0].tags,
        Some(vec![DiagnosticTag::UNNECESSARY]),
        "the 6133 carrier tag must publish as Unnecessary, got: {:?}",
        result[0].tags
    );
    assert_eq!(
        result[1].tags,
        Some(vec![DiagnosticTag::DEPRECATED]),
        "the deprecated carrier tag must publish as Deprecated, got: {:?}",
        result[1].tags
    );
}

/// A SINGLE diagnostic carrying BOTH carrier tags publishes BOTH LSP
/// `DiagnosticTag`s — an unused deprecated import is faded AND struck through.
/// The native LSP tag taxonomy is per-diagnostic-multi-tag, so the merge must
/// not collapse a two-tag carrier to one tag.
#[test]
fn merge_diagnostics_propagates_both_tags_on_single_diagnostic() {
    use crate::type_provider::protocol::TypeDiagnosticTag;

    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let types = vec![TypeDiagnostic {
        message: "'msg' is declared but its value is never read.".to_string(),
        severity: TypeDiagnosticSeverity::Hint,
        start: 6, // TSX offset for "msg"
        end: 9,
        code: Some("6133".to_string()),
        tags: vec![
            TypeDiagnosticTag::Unnecessary,
            TypeDiagnosticTag::Deprecated,
        ],
        related_information: Vec::new(),
    }];

    let result = merge_diagnostics(
        vec![],
        types,
        "App.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );
    assert_eq!(
        result.len(),
        1,
        "the diagnostic maps and survives: {result:?}"
    );
    assert_eq!(
        result[0].tags,
        Some(vec![DiagnosticTag::UNNECESSARY, DiagnosticTag::DEPRECATED]),
        "a two-tag carrier must publish BOTH LSP tags in order, got: {:?}",
        result[0].tags
    );
}

/// Negative: a tagless carrier yields `tags == None` (no spurious fade).
#[test]
fn merge_diagnostics_tagless_input_yields_no_tags() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let types = vec![TypeDiagnostic {
        message: "Type 'number' is not assignable to type 'string'".to_string(),
        severity: TypeDiagnosticSeverity::Error,
        start: 6,
        end: 9,
        code: Some("2322".to_string()),
        tags: Vec::new(),
        related_information: Vec::new(),
    }];

    let result = merge_diagnostics(
        vec![],
        types,
        "App.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].tags, None,
        "a non-6133 / untagged diagnostic must NOT carry a tag"
    );
}

/// @ai-generated — Type diagnostics in unmapped regions are filtered out
#[test]
fn merge_diagnostics_filters_unmapped() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = vec![];
    // Offset 100 is beyond the TSX source
    let types = vec![TypeDiagnostic {
        message: "error in generated code".to_string(),
        severity: TypeDiagnosticSeverity::Error,
        start: 100,
        end: 110,
        code: None,
        tags: Vec::new(),
        related_information: Vec::new(),
    }];

    let result = merge_diagnostics(
        verter,
        types,
        "App.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );
    assert!(result.is_empty(), "unmapped diagnostics should be filtered");
}

/// A diagnostic's `relatedInformation` secondary spans (the "see declaration
/// here" links TS attaches, e.g. the duplicate-identifier "also declared here"
/// span) survive the merge: each related span is mapped back to a carrier
/// `Location` through the SAME current-file mapper the primary diagnostic uses.
///
/// Discriminating on three fronts:
/// - the MAPPABLE related entry (current-TSX offsets that map to the carrier
///   "msg") is published with a real carrier range (line 5, NOT `Range::default`)
///   and the carrier URI;
/// - the UNMAPPABLE related entry (offsets past the TSX) is DROPPED fail-closed
///   (never a line-0 link), so exactly ONE related entry survives;
/// - the primary diagnostic itself survives regardless.
///
/// Pre-fix this does not compile (`TypeDiagnostic` has no `related_information`
/// field and `merge_diagnostics` does not map it), and behaviourally the old
/// `..Default::default()` left `related_information: None` — so the assertions
/// fail against the pre-fix tree.
#[test]
fn merge_diagnostics_maps_related_information_fail_closed() {
    use crate::type_provider::protocol::DiagnosticRelatedInfo;

    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    // The current generated TSX path: its carrier stem (`App.vue`) is a carrier,
    // so `is_carrier_ide_path` routes a related span in THIS file through the
    // in-context mapper (the same-file branch of `resolve_carrier_ide_range_strict`).
    let current_tsx_path = "App.vue.tsx";
    let types = vec![TypeDiagnostic {
        message: "Duplicate identifier 'msg'.".to_string(),
        severity: TypeDiagnosticSeverity::Error,
        start: 6, // TSX offset for "msg" (maps to carrier line 5)
        end: 9,
        code: Some("2300".to_string()),
        tags: Vec::new(),
        related_information: vec![
            // MAPPABLE: a same-file related span over "msg" → carrier line 5.
            DiagnosticRelatedInfo {
                path: current_tsx_path.to_string(),
                start: 6,
                end: 9,
                message: "'msg' was also declared here.".to_string(),
            },
            // UNMAPPABLE: offsets past the TSX source → dropped fail-closed.
            DiagnosticRelatedInfo {
                path: current_tsx_path.to_string(),
                start: 1000,
                end: 1010,
                message: "unmappable related span".to_string(),
            },
        ],
    }];

    let result = merge_diagnostics(
        vec![],
        types,
        current_tsx_path,
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );

    assert_eq!(
        result.len(),
        1,
        "the primary diagnostic must survive: {result:?}"
    );
    let related = result[0]
        .related_information
        .as_ref()
        .expect("the mappable related span must publish a related_information list");
    assert_eq!(
        related.len(),
        1,
        "exactly the mappable related span survives; the unmappable one is dropped fail-closed, got: {related:?}"
    );
    let entry = &related[0];
    assert_eq!(entry.message, "'msg' was also declared here.");
    assert!(
        entry.location.uri.as_str().ends_with("App.vue"),
        "the related location maps back onto the carrier URI, got: {}",
        entry.location.uri.as_str()
    );
    assert_eq!(
        entry.location.range.start.line, 5,
        "the related span maps to the carrier script line (5), not a line-0 default, got: {:?}",
        entry.location.range
    );
    assert_ne!(
        entry.location.range,
        Range::default(),
        "a mapped related range must never collapse to Range::default()"
    );
}

/// NEGATIVE / fail-closed: a primary diagnostic whose related spans are ALL
/// unmappable still publishes — the primary is never dropped because a secondary
/// link failed to map — and its `related_information` is `None` (no degenerate
/// line-0 link sneaks through).
#[test]
fn merge_diagnostics_primary_survives_all_unmappable_related() {
    use crate::type_provider::protocol::DiagnosticRelatedInfo;

    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let current_tsx_path = "App.vue.tsx";
    let types = vec![TypeDiagnostic {
        message: "Type 'number' is not assignable to type 'string'".to_string(),
        severity: TypeDiagnosticSeverity::Error,
        start: 6,
        end: 9,
        code: Some("2322".to_string()),
        tags: Vec::new(),
        related_information: vec![DiagnosticRelatedInfo {
            path: current_tsx_path.to_string(),
            start: 1000,
            end: 1010,
            message: "the expected type comes from here".to_string(),
        }],
    }];

    let result = merge_diagnostics(
        vec![],
        types,
        current_tsx_path,
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );

    assert_eq!(
        result.len(),
        1,
        "the primary diagnostic must publish even when every related span is unmappable: {result:?}"
    );
    assert!(
        result[0].related_information.is_none(),
        "an all-unmappable related set yields no related_information (never a line-0 link), got: {:?}",
        result[0].related_information
    );
}

// ── Definition merge tests ─────────────────────────────────────

fn test_doc_uri() -> Uri {
    "file:///test.vue".parse().unwrap()
}

/// Build an in-memory external source fixture for the definition/type-definition merge
/// path: a synthetic forward-slash path (with `suffix`) plus an [`ExternalSourceReader`]
/// that returns the content for that exact path, modeling the host VFS the production
/// merge reads through (`VerterHost::workspace_read().read_file` → `WorkspaceRead::read_file`).
/// Definition targets carry byte offsets into their own source, so the reader hands that
/// exact source back for the offset→line:col conversion — no disk I/O.
fn ext_source(suffix: &str, content: &str) -> (String, impl Fn(&str) -> Option<Arc<str>>) {
    let path = format!("/virtual/external{suffix}");
    let content: Arc<str> = Arc::from(content);
    let reader_path = path.clone();
    let reader = move |p: &str| (p == reader_path.as_str()).then(|| content.clone());
    (path, reader)
}

/// Reader for cases that never reach the external-source path (empty type defs,
/// verter-preferred, or `.vue.tsx`/`.vue.d.ts` targets resolved before/without a source
/// read): always `None`. Passing it documents that no external source is consulted.
fn no_external_source(_path: &str) -> Option<Arc<str>> {
    None
}

/// @ai-generated — Verter definition is preferred when no type definitions
#[test]
fn merge_definitions_verter_only() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = Some(GotoDefinitionResponse::Scalar(Location {
        uri: "file:///test.vue".parse().unwrap(),
        range: Range::default(),
    }));

    let result = merge_definitions(
        verter,
        vec![],
        "",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &test_doc_uri(),
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );
    assert!(result.is_some());
}

/// @ai-generated — Type definitions used when verter has none
#[test]
fn merge_definitions_type_only() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    // A real external `.ts` whose byte offsets index its own source. `sym` sits on
    // line 1, so a faithful resolve lands on line 1 (not the old line-0 default).
    let source = "export {}\nexport const sym = 1\n";
    let off = source.find("sym").unwrap() as u32;
    let (ts_path, read_source) = ext_source(".ts", source);
    let types = vec![TypeLocation {
        path: ts_path.clone(),
        start: off,
        end: off + 3,
    }];

    let result = merge_definitions(
        None,
        types,
        "",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &test_doc_uri(),
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &read_source,
    );
    match result {
        Some(GotoDefinitionResponse::Scalar(loc)) => {
            assert!(loc.uri.as_str().ends_with(".ts"));
            assert_eq!(
                loc.range.start.line, 1,
                "external target must resolve to the real symbol line, not line 0"
            );
        }
        other => panic!("expected a resolved external definition, got {other:?}"),
    }
}

/// @ai-generated — Neither source returns None
#[test]
fn merge_definitions_neither() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let result = merge_definitions(
        None,
        vec![],
        "",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &test_doc_uri(),
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );
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
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = Some(vec![Location {
        uri: "file:///test.vue".parse().unwrap(),
        range: Range::default(),
    }]);
    let type_refs = vec![TypeLocation {
        path: "/project/utils.ts".to_string(),
        start: 0,
        end: 10,
    }];

    // The external `.ts` ref's byte offsets are converted against its own source (read through
    // the injected VFS reader) — so the cross-file ref survives alongside the verter ref.
    let utils_src: Arc<str> = Arc::from("export const formatCount = 1;\n");
    let read_source = |p: &str| (p == "/project/utils.ts").then(|| utils_src.clone());

    let result = merge_references(
        verter,
        type_refs,
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &read_source,
    );
    assert!(result.is_some());
    assert_eq!(result.unwrap().len(), 2);
}

/// @ai-generated — Empty refs from both returns None
#[test]
fn merge_references_neither() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let result = merge_references(
        None,
        vec![],
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
    );
    assert!(result.is_none());
}

/// @ai-generated — Verter-only refs returned as-is
#[test]
fn merge_references_verter_only() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = Some(vec![Location {
        uri: "file:///test.vue".parse().unwrap(),
        range: Range::default(),
    }]);

    let result = merge_references(
        verter,
        vec![],
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
    );
    assert!(result.is_some());
    assert_eq!(result.unwrap().len(), 1);
}

/// A reference at a FOREIGN carrier IDE `.tsx` (a different component than the one being queried)
/// whose offsets HAPPEN to map in the CURRENT request's mapper must FAIL CLOSED with no external
/// resolver — the offsets index the foreign file's TSX, so the current sourcemap would land on an
/// unrelated location. Discriminating: a lenient resolver that fell back to the current mapper for
/// ANY path would let offsets 6..9 (which map to the current `.vue` `const msg`) produce a bogus
/// foreign reference; the strict resolver drops it.
#[test]
fn merge_references_foreign_carrier_fails_closed_without_resolver() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let foreign = vec![TypeLocation {
        path: "/other.vue.tsx".to_string(),
        start: 6,
        end: 9,
    }];
    let no_external: Option<ExternalIdeResolver> = None;
    let dropped = merge_references(
        None,
        foreign,
        // current request is /test.vue.tsx; the ref targets a DIFFERENT carrier file.
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
    );
    assert!(
        dropped.is_none(),
        "a FOREIGN carrier reference with no resolver must be DROPPED, never mapped through the \
         current request's sourcemap: {dropped:?}"
    );

    // Positive control: the SAME offsets at the SAME (current) carrier file still map.
    let same_file = vec![TypeLocation {
        path: "/test.vue.tsx".to_string(),
        start: 6,
        end: 9,
    }];
    let kept = merge_references(
        None,
        same_file,
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None::<ExternalIdeResolver>,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
    );
    let locs = kept.expect("the same-file carrier reference must survive");
    assert_eq!(locs.len(), 1, "the same-file carrier reference survives");
    assert_eq!(
        locs[0].range.start,
        Position {
            line: 5,
            character: 6,
        },
        "the same-file reference maps to the carrier `const msg`, got {:?}",
        locs[0].range.start
    );
}

// ── Document highlights merge tests ───────────────────────────────

/// @ai-generated — Type highlights mapped and merged with verter highlights
#[test]
fn merge_highlights_both_present() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
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

    let result = merge_document_highlights(verter, type_highlights, &tsx_li, &mapper, &carrier_li);
    assert!(result.is_some());
    // Should be 1 (deduplicated since both point to line 5, col 6)
    assert_eq!(result.unwrap().len(), 1);
}

/// @ai-generated — Neither highlights returns None
#[test]
fn merge_highlights_neither() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let result = merge_document_highlights(None, vec![], &tsx_li, &mapper, &carrier_li);
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
                label: protocol::ParameterLabelKind::Simple("x".to_string()),
                documentation: Some("The number param".to_string()),
            }],
            active_parameter: None,
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

/// Per-signature active parameter and the offset-form parameter label survive
/// the carrier → LSP conversion.
///
/// Discriminates against the pre-K2 merge, which (a) hard-coded per-signature
/// `active_parameter: None` and (b) only ever emitted `ParameterLabel::Simple`
/// (the `ParameterLabelKind::Offsets` carrier form and the `SignatureInfo`
/// `active_parameter` field did not exist). Both assertions below fail on the
/// pre-fix tree.
#[test]
fn merge_signature_help_carries_active_param_and_offset_label() {
    use tower_lsp_server::ls_types::ParameterLabel;

    let sig = protocol::SignatureHelp {
        signatures: vec![protocol::SignatureInfo {
            label: "greet(name: string, times: number): void".to_string(),
            documentation: None,
            parameters: vec![
                protocol::ParameterInfo {
                    // "name: string" spans UTF-16 [6, 18) of the label above.
                    label: protocol::ParameterLabelKind::Offsets(6, 18),
                    documentation: None,
                },
                protocol::ParameterInfo {
                    label: protocol::ParameterLabelKind::Offsets(20, 33),
                    documentation: None,
                },
            ],
            active_parameter: Some(1),
        }],
        active_signature: Some(0),
        active_parameter: Some(1),
    };

    let help = merge_signature_help(Some(sig)).expect("present");
    assert_eq!(help.signatures.len(), 1);
    let s0 = &help.signatures[0];
    // (b) per-signature active parameter is carried (was hard-coded None pre-fix).
    assert_eq!(
        s0.active_parameter,
        Some(1),
        "per-signature active_parameter must be carried through the merge"
    );
    let params = s0.parameters.as_ref().expect("parameters present");
    assert_eq!(params.len(), 2);
    // (a) offset-form labels map to LabelOffsets (was always Simple pre-fix).
    assert_eq!(
        params[0].label,
        ParameterLabel::LabelOffsets([6, 18]),
        "Offsets(6,18) must map to ParameterLabel::LabelOffsets([6,18])"
    );
    assert_eq!(params[1].label, ParameterLabel::LabelOffsets([20, 33]));
    // top-level signals stay as-is.
    assert_eq!(help.active_signature, Some(0));
    assert_eq!(help.active_parameter, Some(1));
}

/// A `Simple` carrier label still maps to `ParameterLabel::Simple` (fail-closed
/// passthrough form — e.g. a tgo provider that sends string labels).
#[test]
fn merge_signature_help_simple_label_passthrough() {
    use tower_lsp_server::ls_types::ParameterLabel;

    let sig = protocol::SignatureHelp {
        signatures: vec![protocol::SignatureInfo {
            label: "fn(x: number): void".to_string(),
            documentation: None,
            parameters: vec![protocol::ParameterInfo {
                label: protocol::ParameterLabelKind::Simple("x: number".to_string()),
                documentation: None,
            }],
            active_parameter: None,
        }],
        active_signature: Some(0),
        active_parameter: Some(0),
    };

    let help = merge_signature_help(Some(sig)).expect("present");
    let params = help.signatures[0].parameters.as_ref().expect("params");
    assert_eq!(
        params[0].label,
        ParameterLabel::Simple("x: number".to_string())
    );
    assert_eq!(help.signatures[0].active_parameter, None);
}

// ── Code actions merge tests ──────────────────────────────────────

/// @ai-generated — Code actions with mappable edits are returned
#[test]
fn merge_code_actions_with_edits() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
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

    let no_external: Option<ExternalIdeResolver> = None;
    let result = merge_code_actions(
        actions,
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
        "",
        &[],
    );
    assert_eq!(result.len(), 1);
}

/// ISSUE-8: a "Remove unused declaration" action whose edit DELETES a carrier-IDE
/// TSX span (empty `new_text`) maps back to the `.vue` SOURCE range — the deletion
/// targets the real decl, never a line-0 mis-map. A second edit fragment that
/// can't map is dropped (fail-closed), and the action survives on its mappable
/// edit.
#[test]
fn merge_code_actions_remove_unused_deletion_maps_back_to_vue_source() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // The TSX `msg` identifier sits at byte offset 6..9 (line 0, cols 6..9) and
    // maps to the Vue `<script setup>` `const msg` on line 5, cols 6..9.
    let actions = vec![TypeCodeAction {
        title: "Remove unused declaration".to_string(),
        kind: Some("quickfix".to_string()),
        edits: vec![
            protocol::TypeCodeEdit {
                path: "/test.vue.tsx".to_string(),
                start: 6,
                end: 9,
                new_text: String::new(),
            },
            // An unmappable carrier-IDE fragment (no token covers offset 999) must
            // be DROPPED, not line-0'd.
            protocol::TypeCodeEdit {
                path: "/test.vue.tsx".to_string(),
                start: 999,
                end: 1002,
                new_text: String::new(),
            },
        ],
    }];

    let no_external: Option<ExternalIdeResolver> = None;
    let result = merge_code_actions(
        actions,
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
        "",
        &[],
    );
    assert_eq!(
        result.len(),
        1,
        "the remove-unused action must survive map-back"
    );
    let CodeActionOrCommand::CodeAction(action) = &result[0] else {
        panic!("expected a CodeAction");
    };
    assert_eq!(action.title, "Remove unused declaration");
    let changes = action.edit.as_ref().unwrap().changes.as_ref().unwrap();

    // F4: the deletion must be keyed by the `.vue` SOURCE URI — never the generated
    // `.tsx`. A mutant that left the edit on `/test.vue.tsx` now fails here.
    let (change_uri, edits) = changes
        .iter()
        .next()
        .expect("the action carries one change set");
    assert_eq!(
        change_uri.as_str(),
        "file:///test.vue",
        "the deletion must be keyed by the .vue source URI, got {change_uri:?}"
    );
    assert!(
        !change_uri.as_str().ends_with(".tsx"),
        "the deletion must NOT target the generated .tsx, got {change_uri:?}"
    );
    assert_eq!(
        edits.len(),
        1,
        "the unmappable fragment must be dropped, leaving only the mapped deletion"
    );
    assert!(
        edits[0].new_text.is_empty(),
        "a remove-unused deletion has empty new_text, got {:?}",
        edits[0].new_text
    );
    // F4: assert BOTH endpoints of the mapped carrier range — `const msg` (the TSX
    // `msg` at cols 6..9) maps to Vue line 5, cols 6..9. A same-line-but-wrong span
    // (e.g. a start-only assertion let a 6..7 mutant pass) now fails.
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 5,
            character: 6,
        },
        "the deletion start must map to the Vue `const msg` decl on line 5, got {:?}",
        edits[0].range.start
    );
    assert_eq!(
        edits[0].range.end,
        Position {
            line: 5,
            character: 9,
        },
        "the deletion end must map to the end of `msg` on line 5, got {:?}",
        edits[0].range.end
    );
    assert_ne!(
        edits[0].range,
        Range::default(),
        "the deletion must never collapse to (0,0)"
    );
}

/// A cross-file (non-carrier) code-action edit keeps its REAL range, read from the target file's
/// own source — and a target whose source can't be read is FAIL-CLOSED (dropped), never line-0'd.
#[test]
fn merge_code_actions_external_edit_keeps_real_range_or_fails_closed() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // `helper` sits on line 1 of utils.ts; the edit's byte offsets index utils.ts's own source.
    let utils_src = "export {};\nexport const helper = 1;\n";
    let off = utils_src.find("helper").unwrap() as u32;
    let end = off + "helper".len() as u32;
    let utils_li = LineIndex::new_utf16(utils_src);
    let expected_start = utils_li.offset_to_position(off).unwrap();
    assert_eq!(expected_start.line, 1, "fixture precondition");

    let src: Arc<str> = Arc::from(utils_src);
    let read_source = |p: &str| (p == "/proj/utils.ts").then(|| src.clone());

    let actions = vec![TypeCodeAction {
        title: "Rename helper".to_string(),
        kind: Some("refactor".to_string()),
        edits: vec![protocol::TypeCodeEdit {
            path: "/proj/utils.ts".to_string(),
            start: off,
            end,
            new_text: "renamedHelper".to_string(),
        }],
    }];

    let no_external: Option<ExternalIdeResolver> = None;
    let result = merge_code_actions(
        actions,
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &read_source,
        "",
        &[],
    );
    assert_eq!(result.len(), 1, "the external code action must survive");
    let CodeActionOrCommand::CodeAction(action) = &result[0] else {
        panic!("expected a CodeAction");
    };
    let changes = action.edit.as_ref().unwrap().changes.as_ref().unwrap();
    let edits = changes.values().next().unwrap();
    assert_eq!(
        edits[0].range.start, expected_start,
        "external edit start must be the real line:col {expected_start:?}, got {:?}",
        edits[0].range.start
    );
    assert_ne!(
        edits[0].range,
        Range::default(),
        "external code-action edit must never collapse to (0,0)"
    );

    // Now with an unreadable source: the action is dropped entirely (fail-closed).
    let actions = vec![TypeCodeAction {
        title: "Rename helper".to_string(),
        kind: Some("refactor".to_string()),
        edits: vec![protocol::TypeCodeEdit {
            path: "/proj/gone.ts".to_string(),
            start: off,
            end,
            new_text: "renamedHelper".to_string(),
        }],
    }];
    let no_external: Option<ExternalIdeResolver> = None;
    let dropped = merge_code_actions(
        actions,
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
        "",
        &[],
    );
    assert!(
        dropped.is_empty(),
        "a code action whose only edit can't be resolved must be dropped, not line-0'd: {dropped:?}"
    );
}

/// A code-action edit targeting a FOREIGN carrier IDE `.tsx` (a different component than the one
/// being queried) must FAIL CLOSED when no external resolver can supply that file's own sourcemap:
/// the edit's offsets index the foreign file's TSX, so mapping them through the CURRENT request's
/// mapper would corrupt an unrelated location. Discriminating: mapping every `is_carrier_ide_path`
/// edit through the current mapper unconditionally would let a foreign edit with offsets that happen
/// to be mappable in the current sourcemap (6..9 → the current `.vue` `const msg`) produce a bogus
/// carrier edit; the strict resolver drops it.
#[test]
fn merge_code_actions_foreign_carrier_edit_fails_closed_without_resolver() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // Offsets 6..9 ARE mappable in the CURRENT mapper (they map to the current `.vue` `const msg`
    // on line 5). The OLD code would have happily emitted a carrier edit for the FOREIGN file from
    // those offsets — exactly the mis-map this asserts is gone.
    let foreign = vec![TypeCodeAction {
        title: "Fix foreign".to_string(),
        kind: Some("quickfix".to_string()),
        edits: vec![protocol::TypeCodeEdit {
            path: "/other.vue.tsx".to_string(),
            start: 6,
            end: 9,
            new_text: "renamed".to_string(),
        }],
    }];
    let no_external: Option<ExternalIdeResolver> = None;
    let dropped = merge_code_actions(
        foreign,
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
        "",
        &[],
    );
    assert!(
        dropped.is_empty(),
        "a FOREIGN carrier edit with no resolver must be DROPPED, never mapped through the current \
         request's sourcemap: {dropped:?}"
    );

    // Positive control: a SAME-FILE carrier edit (path == current_tsx_path) with the same mappable
    // offsets still maps to the correct carrier range — proving the foreign-drop did not disable the
    // same-file path.
    let same_file = vec![TypeCodeAction {
        title: "Fix self".to_string(),
        kind: Some("quickfix".to_string()),
        edits: vec![protocol::TypeCodeEdit {
            path: "/test.vue.tsx".to_string(),
            start: 6,
            end: 9,
            new_text: "renamed".to_string(),
        }],
    }];
    let no_external: Option<ExternalIdeResolver> = None;
    let kept = merge_code_actions(
        same_file,
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
        "",
        &[],
    );
    assert_eq!(kept.len(), 1, "the same-file carrier edit must survive");
    let CodeActionOrCommand::CodeAction(action) = &kept[0] else {
        panic!("expected a CodeAction");
    };
    let changes = action.edit.as_ref().unwrap().changes.as_ref().unwrap();
    let (change_uri, edits) = changes.iter().next().expect("one change set");
    assert_eq!(
        change_uri.as_str(),
        "file:///test.vue",
        "the same-file edit must be keyed by the .vue source URI, got {change_uri:?}"
    );
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 5,
            character: 6,
        },
        "the same-file edit must map to the Vue `const msg` decl, got {:?}",
        edits[0].range.start
    );
}

/// When an external resolver DOES supply the foreign carrier file's own context, the foreign edit
/// maps through THAT context's sourcemap — never the current request's.
#[test]
fn merge_code_actions_foreign_carrier_edit_maps_through_external_context() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // A second, distinct carrier whose own sourcemap maps TSX offset 6..9 to a DIFFERENT carrier
    // line than the current request's mapper. The foreign edit must land on the foreign line.
    let foreign_carrier =
        "<template>\n  <span/>\n</template>\n\n<script setup>\n\n\nconst far = 1;\n</script>";
    let foreign_tsx = "const far = 1;\n";
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content("Other.vue", foreign_carrier);
    // TSX line 0 → foreign carrier line 7 (NOT line 5 like the current mapper).
    builder.add_token(0, 0, 7, 0, Some(source_id), None);
    builder.add_token(0, 6, 7, 6, Some(source_id), None);
    let foreign_json = builder.into_sourcemap().to_json_string();
    let foreign_mapper =
        ProviderPositionMapper::source_map(PositionMapper::from_json(&foreign_json).unwrap());
    let foreign_carrier_li = LineIndex::new_utf16(foreign_carrier);
    let foreign_tsx_li = LineIndex::new_utf16(foreign_tsx);

    let resolver = |p: &str| -> Option<ExternalIdeContext> {
        (p == "/other.vue.tsx").then(|| ExternalIdeContext {
            tsx_line_index: foreign_tsx_li.clone(),
            mapper: foreign_mapper.clone(),
            carrier_line_index: foreign_carrier_li.clone(),
            carrier_negotiated_line_index: None,
        })
    };
    let ext: Option<ExternalIdeResolver> = Some(&resolver);

    let actions = vec![TypeCodeAction {
        title: "Fix foreign".to_string(),
        kind: Some("quickfix".to_string()),
        edits: vec![protocol::TypeCodeEdit {
            path: "/other.vue.tsx".to_string(),
            start: 6,
            end: 9,
            new_text: "renamed".to_string(),
        }],
    }];
    let result = merge_code_actions(
        actions,
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        ext,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
        "",
        &[],
    );
    assert_eq!(
        result.len(),
        1,
        "the foreign edit maps through its own context"
    );
    let CodeActionOrCommand::CodeAction(action) = &result[0] else {
        panic!("expected a CodeAction");
    };
    let changes = action.edit.as_ref().unwrap().changes.as_ref().unwrap();
    let (change_uri, edits) = changes.iter().next().expect("one change set");
    assert_eq!(
        change_uri.as_str(),
        "file:///other.vue",
        "the foreign edit must be keyed by the FOREIGN .vue URI, got {change_uri:?}"
    );
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 7,
            character: 6,
        },
        "the foreign edit must map through the FOREIGN context (line 7), not the current (line 5): {:?}",
        edits[0].range.start
    );
}

/// A SAME-FILE carrier edit whose path differs from `current_tsx_path` ONLY in
/// spelling the canonical normalizer folds (backslashes vs forward slashes, and
/// drive-letter case) MUST map through the in-context mapper — never be treated
/// as foreign and dropped. The raw `==` discriminator treated
/// `C:\proj\test.vue.tsx` and `c:/proj/test.vue.tsx` as distinct files, so with
/// no external resolver the same-file edit was false-dropped; comparing through
/// the canonical path identity keeps it.
#[test]
fn merge_code_actions_same_file_differently_spelled_path_maps_not_dropped() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // `current_tsx_path` is the canonical spelling; the edit's path is the SAME
    // file spelled with Windows backslashes and an uppercase drive letter. The
    // canonical normalizer folds both to `c:/proj/test.vue.tsx`.
    let current = "c:/proj/test.vue.tsx";
    let edit_same_file_other_spelling = r"C:\proj\test.vue.tsx";

    let actions = vec![TypeCodeAction {
        title: "Remove unused declaration".to_string(),
        kind: Some("quickfix".to_string()),
        edits: vec![protocol::TypeCodeEdit {
            // Offsets 6..9 map (in-context) to the carrier `const msg` on line 5.
            path: edit_same_file_other_spelling.to_string(),
            start: 6,
            end: 9,
            new_text: String::new(),
        }],
    }];

    // No external resolver: the raw-`==` code would treat the differently-spelled
    // path as foreign and, with no resolver, DROP it.
    let no_external: Option<ExternalIdeResolver> = None;
    let kept = merge_code_actions(
        actions,
        current,
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
        "",
        &[],
    );

    assert_eq!(
        kept.len(),
        1,
        "a same-file edit spelled with backslashes / uppercase drive must NOT be \
         dropped as foreign: {kept:?}"
    );
    let CodeActionOrCommand::CodeAction(action) = &kept[0] else {
        panic!("expected a CodeAction");
    };
    let changes = action.edit.as_ref().unwrap().changes.as_ref().unwrap();
    let (change_uri, edits) = changes.iter().next().expect("one change set");
    // The carrier URI must be the canonical `.vue` source (drive lowercased, slashes
    // forward) — proving the canonical spelling is carried downstream consistently.
    assert_eq!(
        change_uri.as_str(),
        "file:///c:/proj/test.vue",
        "the same-file edit must key the canonical .vue source URI, got {change_uri:?}"
    );
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 5,
            character: 6,
        },
        "the same-file edit must map (in-context) to the carrier `const msg`, got {:?}",
        edits[0].range.start
    );
    assert_eq!(
        edits[0].range.end,
        Position {
            line: 5,
            character: 9,
        },
        "the same-file edit end must map to the end of `msg`, got {:?}",
        edits[0].range.end
    );
}

// ── Add-import prelude re-anchor (K3) ─────────────────────────────
//
// A provider `addMissingImport` quickfix inserts a BRAND-NEW import line at the top of the
// generated TSX, which lands inside Verter's synthetic, unmapped helper-import preamble. The strict
// mapper returns `None` for that region, so the edit was historically DROPPED. `merge_code_actions`
// re-anchors such a CURRENT-file preamble insertion at the SFC's `<script setup>` import site via
// the shared completion re-anchor. These tests pin: (1) the prelude insertion now LANDS at a real
// carrier range; (2) a zero-width insertion PAST the preamble boundary stays dropped; (3) a FOREIGN
// carrier `.tsx` prelude insertion stays dropped (no in-context anchor).

/// A carrier-IDE mapper whose generated TSX has a synthetic helper-import preamble on line 0 (which
/// does NOT map to the carrier) and a single mapped user line below it, plus the typed
/// `x_verter_helper_preamble_end` boundary IDE codegen publishes. Returns the carrier source (a real
/// `<script setup>` SFC), its UTF-16 line index, the TSX UTF-16 line index, the mapper, and the
/// SFC-absolute import spans (none — the SFC imports nothing, forcing a brand-new import insertion).
fn make_preamble_mapper_and_indexes() -> (String, LineIndex, LineIndex, ProviderPositionMapper) {
    // Carrier `.vue`: line 0 `<script setup lang="ts">`, line 1 blank, line 2 `const base = ...`.
    let carrier_source = "<script setup lang=\"ts\">\n\nconst base = 1;\n</script>\n<template>\n  <div>{{ base }}</div>\n</template>\n".to_string();
    // Generated TSX: line 0 is the synthetic helper preamble (unmapped), line 1 is the user decl
    // (mapped), line 2 is TRAILING synthetic component/export code (unmapped, PAST the preamble).
    let tsx_source =
        "import { defineComponent } from 'vue';\nconst base = 1;\nexport default {};\n";

    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content("App.vue", &carrier_source);
    // Map ONLY the user line: TSX line 1 col 0 → carrier line 2 col 0 (`const base`). Line 0 (the
    // preamble) and line 2 (trailing synthetic) are deliberately left UNMAPPED, so an insertion at
    // TSX offset 0 (preamble) or on line 2 (trailing) both miss the strict mapper.
    builder.add_token(1, 0, 2, 0, Some(source_id), None);
    builder.add_token(1, 6, 2, 6, Some(source_id), None);
    let base_json = builder.into_sourcemap().to_json_string();

    // Inject the `x_verter_helper_preamble_end` member IDE codegen emits (oxc drops unknown members
    // on serialize). The boundary is the generated position immediately after the last helper import
    // — here, the start of the user line (line 1, col 0). This is fixture JSON, not generated code
    // output, so direct `serde_json` assembly is appropriate (no CodeTransform involved).
    let mut value: serde_json::Value = serde_json::from_str(&base_json).unwrap();
    value["x_verter_helper_preamble_end"] = serde_json::json!({ "line": 1, "character": 0 });
    let json = serde_json::to_string(&value).unwrap();

    let mapper = ProviderPositionMapper::source_map(PositionMapper::from_json(&json).unwrap());
    let carrier_li = LineIndex::new_utf16(&carrier_source);
    let tsx_li = LineIndex::new_utf16(tsx_source);
    (carrier_source, carrier_li, tsx_li, mapper)
}

/// A CURRENT-file `addMissingImport` quickfix whose zero-width edit inserts a new import at the
/// synthetic TSX preamble (offset 0) — unmapped by the strict mapper — is RE-ANCHORED at the SFC's
/// `<script setup>` import site, NOT dropped. Pre-fix this action had no surviving edit (the prelude
/// edit was dropped) and `merge_code_actions` returned an empty vec.
#[test]
fn merge_code_actions_add_import_prelude_insertion_reanchors_to_script_setup() {
    let (carrier_source, carrier_li, tsx_li, mapper) = make_preamble_mapper_and_indexes();

    // Precondition: TSX offset 0 does NOT map through the strict mapper (it is in the preamble).
    assert!(
        tsx_range_to_carrier_range(0, 0, &tsx_li, &mapper, &carrier_li).is_none(),
        "fixture precondition: the preamble offset 0 must be unmapped"
    );

    let actions = vec![TypeCodeAction {
        title: "Add import from \"vue\"".to_string(),
        kind: Some("quickfix".to_string()),
        edits: vec![protocol::TypeCodeEdit {
            path: "/test.vue.tsx".to_string(),
            start: 0,
            end: 0,
            new_text: "import { computed } from \"vue\";\n".to_string(),
        }],
    }];

    let no_external: Option<ExternalIdeResolver> = None;
    let result = merge_code_actions(
        actions,
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
        &carrier_source,
        &[],
    );

    assert_eq!(
        result.len(),
        1,
        "the add-import action must survive via the prelude re-anchor, not be dropped: {result:?}"
    );
    let CodeActionOrCommand::CodeAction(action) = &result[0] else {
        panic!("expected a CodeAction");
    };
    let changes = action.edit.as_ref().unwrap().changes.as_ref().unwrap();
    let (change_uri, edits) = changes.iter().next().expect("one change set");
    // The re-anchored import must key the `.vue` SOURCE URI (never the generated `.tsx`).
    assert_eq!(
        change_uri.as_str(),
        "file:///test.vue",
        "the add-import edit must be keyed by the .vue source URI, got {change_uri:?}"
    );
    assert!(
        !change_uri.as_str().ends_with(".tsx"),
        "the add-import edit must NOT target the generated .tsx, got {change_uri:?}"
    );
    assert_eq!(edits.len(), 1, "exactly one re-anchored import edit");
    // The import text is carried verbatim.
    assert_eq!(
        edits[0].new_text, "import { computed } from \"vue\";\n",
        "the re-anchored edit must carry the provider's import text"
    );
    // It must NOT collapse to line 0 / Range::default(). The `<script setup lang="ts">` tag is on
    // carrier line 0; `resolve_script_import_anchor` inserts at the script CONTENT start (line 1,
    // past the one leading break), so the re-anchored range is a zero-width insertion ON line 1.
    assert_ne!(
        edits[0].range,
        Range::default(),
        "the re-anchored import must never collapse to (0,0)"
    );
    assert_eq!(
        edits[0].range.start, edits[0].range.end,
        "an import insertion is a zero-width edit"
    );
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 1,
            character: 0,
        },
        "the re-anchored import must land at the <script setup> content start (line 1), got {:?}",
        edits[0].range.start
    );
}

/// A zero-width insertion PAST the published preamble boundary (in trailing synthetic
/// component/export code, not the helper-import preamble) is NOT a re-anchorable import insertion —
/// it stays DROPPED, exactly as a non-preamble mapper miss does today. Discriminating: a blanket
/// "any unmapped zero-width edit re-anchors" would wrongly resurrect this as a bogus import.
#[test]
fn merge_code_actions_zero_width_insertion_past_preamble_is_dropped() {
    let (carrier_source, carrier_li, tsx_li, mapper) = make_preamble_mapper_and_indexes();

    // TSX line 2 is the trailing synthetic `export default {};` — UNMAPPED (strict mapper returns
    // None) and PAST the preamble-end boundary at line 1 col 0. So a zero-width insertion there
    // reaches the re-anchor branch (mapper miss) but `is_preamble_import_insertion` must reject it
    // (past the boundary), leaving it dropped.
    let past_preamble_offset = tsx_li
        .position_to_offset(&Position {
            line: 2,
            character: 0,
        })
        .expect("a valid TSX offset on the trailing synthetic line");
    // Precondition: this offset does NOT map through the strict mapper (so the drop is the
    // re-anchor branch's decision, not a normal mapped edit).
    assert!(
        tsx_range_to_carrier_range(
            past_preamble_offset,
            past_preamble_offset,
            &tsx_li,
            &mapper,
            &carrier_li
        )
        .is_none(),
        "fixture precondition: the trailing synthetic offset must be unmapped"
    );

    let actions = vec![TypeCodeAction {
        title: "Add import past preamble".to_string(),
        kind: Some("quickfix".to_string()),
        edits: vec![protocol::TypeCodeEdit {
            path: "/test.vue.tsx".to_string(),
            start: past_preamble_offset,
            end: past_preamble_offset,
            new_text: "import { computed } from \"vue\";\n".to_string(),
        }],
    }];

    let no_external: Option<ExternalIdeResolver> = None;
    let dropped = merge_code_actions(
        actions,
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
        &carrier_source,
        &[],
    );
    assert!(
        dropped.is_empty(),
        "a zero-width insertion PAST the preamble boundary must stay dropped, never re-anchored: \
         {dropped:?}"
    );
}

/// A FOREIGN carrier `.tsx` (a different component than the one being queried) preamble insertion is
/// NOT re-anchored: the in-context carrier source / `<script setup>` anchor belong to the CURRENT
/// file, not the foreign one, so re-anchoring it would splice an import into the WRONG `.vue`. With
/// no external resolver it stays DROPPED. Discriminating: the offset 0 IS a preamble insertion in
/// the current mapper, so a path-agnostic re-anchor would wrongly land it on the current `.vue`.
#[test]
fn merge_code_actions_foreign_carrier_prelude_insertion_is_dropped() {
    let (carrier_source, carrier_li, tsx_li, mapper) = make_preamble_mapper_and_indexes();

    let foreign = vec![TypeCodeAction {
        title: "Add import (foreign)".to_string(),
        kind: Some("quickfix".to_string()),
        edits: vec![protocol::TypeCodeEdit {
            // A DIFFERENT component's generated file — not `current_tsx_path`.
            path: "/other.vue.tsx".to_string(),
            start: 0,
            end: 0,
            new_text: "import { computed } from \"vue\";\n".to_string(),
        }],
    }];

    let no_external: Option<ExternalIdeResolver> = None;
    let dropped = merge_code_actions(
        foreign,
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
        &carrier_source,
        &[],
    );
    assert!(
        dropped.is_empty(),
        "a FOREIGN carrier prelude insertion must be DROPPED — the current request's <script setup> \
         anchor is the wrong file to receive it: {dropped:?}"
    );
}

/// @ai-generated — Empty actions returns empty vec
#[test]
fn merge_code_actions_empty() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let no_external: Option<ExternalIdeResolver> = None;
    let result = merge_code_actions(
        vec![],
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
        "",
        &[],
    );
    assert!(result.is_empty());
}

// ── Semantic tokens merge tests ───────────────────────────────────

/// @ai-generated — Semantic tokens mapped from TSX to Vue
#[test]
fn merge_semantic_tokens_basic() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    // Token at TSX offset 6 (= "msg"), length 3
    let tokens = vec![protocol::SemanticToken {
        start: 6,
        length: 3,
        token_type: 8, // VARIABLE
        token_modifiers: 0,
    }];

    let result = merge_semantic_tokens(tokens, &tsx_li, &mapper, &carrier_li);
    assert_eq!(result.len(), 1);
    // Should map to Vue line 5, col 6
    assert_eq!(result[0].length, 3);
    assert_eq!(result[0].token_type, 8);
}

/// @ai-generated — Empty tokens returns empty vec
#[test]
fn merge_semantic_tokens_empty() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let result = merge_semantic_tokens(vec![], &tsx_li, &mapper, &carrier_li);
    assert!(result.is_empty());
}

/// Semantic token length should be computed in Vue coordinates by mapping both
/// start and end positions, not by preserving the TSX length verbatim.
/// When the TSX text differs in length from Vue text (e.g., `__props` vs `$props`),
/// the raw TSX length would be wrong in Vue space.
#[test]
fn merge_semantic_tokens_length_via_end_mapping() {
    // Vue: line 5 has `const msg = "hello";` (col 6 = 'msg', length 3)
    // TSX: line 0 has `const msg = "hello";` (col 6 = 'msg', length 3)
    // In this case TSX and Vue lengths match, but the mechanism should map end too.
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    let tokens = vec![protocol::SemanticToken {
        start: 6,  // TSX offset of 'msg'
        length: 3, // length in TSX = 3
        token_type: 8,
        token_modifiers: 0,
    }];

    let result = merge_semantic_tokens(tokens, &tsx_li, &mapper, &carrier_li);
    assert_eq!(result.len(), 1);
    // Both start AND end should be mapped — length should be 3 in Vue coordinates
    assert_eq!(
        result[0].length, 3,
        "length should be correct in Vue coordinates"
    );
}

/// Token whose end position maps to a different line should be filtered out.
#[test]
fn merge_semantic_tokens_cross_line_filtered() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // TSX: "const msg = \"hello\";\n" (20 chars)
    // Token spanning from col 0 with excessive length that crosses line boundary
    let tokens = vec![protocol::SemanticToken {
        start: 0,
        length: 100, // way past end of line — would cross line boundaries
        token_type: 8,
        token_modifiers: 0,
    }];

    let result = merge_semantic_tokens(tokens, &tsx_li, &mapper, &carrier_li);
    // Should be filtered out because end position mapping crosses line or is out of bounds
    // (or length should be clamped to line end)
    if !result.is_empty() {
        assert!(
            result[0].length < 100,
            "excessive length should be clamped or token filtered, got length {}",
            result[0].length
        );
    }
}

// ── Rename merge tests ────────────────────────────────────────────

/// @ai-generated — Verter-only rename returns as-is
#[test]
fn merge_rename_verter_only() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
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

    let result = merge_rename_locations(
        verter,
        vec![],
        "newName",
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        None,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
    );
    assert!(result.is_some());
}

/// @ai-generated — Empty rename from both returns None
#[test]
fn merge_rename_neither() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let result = merge_rename_locations(
        None,
        vec![],
        "newName",
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        None,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
    );
    assert!(result.is_none());
}

/// A rename location at a FOREIGN carrier IDE `.tsx` whose offsets HAPPEN to map in the CURRENT
/// request's mapper must FAIL CLOSED with no external resolver — the offsets index the foreign
/// file's TSX, so the current sourcemap would write the new name at an unrelated location and
/// CORRUPT it. Discriminating: a lenient resolver that fell back to the current mapper for ANY path
/// would write at the bogus location; the strict resolver drops it. This is the rename twin of the
/// code-action/references foreign drop — the corruption stakes are highest here because rename
/// produces WRITE edits.
#[test]
fn merge_rename_foreign_carrier_fails_closed_without_resolver() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let foreign = vec![RenameLocation {
        path: "/other.vue.tsx".to_string(),
        start: 6,
        end: 9,
    }];
    let no_external: Option<ExternalIdeResolver> = None;
    let dropped = merge_rename_locations(
        None,
        foreign,
        "renamed",
        // current request is /test.vue.tsx; the rename targets a DIFFERENT carrier file.
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        no_external,
        None::<ExternalApiResolver>,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
    );
    assert!(
        dropped.is_none(),
        "a FOREIGN carrier rename edit with no resolver must be DROPPED — a line-0/wrong-file \
         rename WRITE would corrupt an unrelated file: {dropped:?}"
    );

    // Positive control: the SAME offsets at the SAME (current) carrier file still produce the
    // mapped rename edit, proving the strict change did not over-drop the same-file path.
    let same_file = vec![RenameLocation {
        path: "/test.vue.tsx".to_string(),
        start: 6,
        end: 9,
    }];
    let kept = merge_rename_locations(
        None,
        same_file,
        "renamed",
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None::<ExternalIdeResolver>,
        None::<ExternalApiResolver>,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
    );
    let edit = kept.expect("the same-file carrier rename must survive");
    let changes = edit.changes.expect("rename produces changes");
    let (uri, edits) = changes.iter().next().expect("one change set");
    assert_eq!(
        uri.as_str(),
        "file:///test.vue",
        "the same-file rename must key the .vue source URI, got {uri:?}"
    );
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 5,
            character: 6,
        },
        "the same-file rename maps to the carrier `const msg`, got {:?}",
        edits[0].range.start
    );
}

// ── Definition merge tests (Bug 2) ───────────────────────────────

/// A `.vue.tsx` target that IS the file being queried (`loc.path == current_tsx_path`)
/// maps its byte offsets back to Vue through the in-context mapper — no external resolver
/// needed, and never the old `Range::default()` (0,0) collapse.
#[test]
fn merge_definitions_maps_current_file_carrier_tsx_to_carrier_positions() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // The current document's generated TSX. TSX offset 6..9 = "msg" (in "const msg = ..."),
    // which the in-context mapper carries back to Vue line 5, col 6..9.
    let current_tsx_path = "/home/user/App.vue.tsx";
    let type_defs = vec![TypeLocation {
        path: current_tsx_path.to_string(),
        start: 6,
        end: 9,
    }];

    let result = merge_definitions(
        None,
        type_defs,
        current_tsx_path,
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &test_doc_uri(),
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );
    assert!(result.is_some(), "Expected definition response");

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(loc) => {
            // URI should point to .vue (not .vue.tsx)
            assert!(
                loc.uri.as_str().ends_with("App.vue"),
                "URI should be .vue, got: {}",
                loc.uri.as_str()
            );
            // Exact full range: "msg" at Vue line 5, cols 6..9 — not the (0,0) default,
            // and not just "some non-zero line".
            assert_ne!(
                loc.range,
                Range::default(),
                "current-file .vue.tsx range must not collapse to (0,0)"
            );
            assert_eq!(
                loc.range,
                Range {
                    start: Position {
                        line: 5,
                        character: 6,
                    },
                    end: Position {
                        line: 5,
                        character: 9,
                    },
                },
                "expected exact Vue range (5,6)..(5,9) for 'msg'"
            );
        }
        _ => panic!("Unexpected definition response type"),
    }
}

/// A non-`.vue` target keeps its own URI (no normalization) AND resolves its byte
/// offsets against its own source to a real `Range` — not the old line-0 default.
#[test]
fn merge_definitions_non_carrier_target_resolves_real_range() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // `helper` is on line 2 of the fixture, so a faithful resolve lands on line 2.
    let source = "export {}\n\nexport function helper() {}\n";
    let off = source.find("helper").unwrap() as u32;
    let (ts_path, read_source) = ext_source(".ts", source);
    let type_defs = vec![TypeLocation {
        path: ts_path.clone(),
        start: off,
        end: off + 6,
    }];

    let result = merge_definitions(
        None,
        type_defs,
        "",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &test_doc_uri(),
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &read_source,
    );
    match result {
        Some(GotoDefinitionResponse::Scalar(loc)) => {
            // URI passes through unchanged (a non-`.vue` target is not normalized).
            assert!(
                loc.uri.as_str().ends_with(".ts") && !loc.uri.as_str().contains(".vue"),
                "external .ts URI should pass through unchanged, got: {}",
                loc.uri.as_str()
            );
            // Range resolves to the real symbol line, not the old (0,0) default.
            assert_eq!(loc.range.start.line, 2, "must land on the real symbol line");
            assert_ne!(loc.range, Range::default(), "must not collapse to line 0");
        }
        other => panic!("expected a resolved external definition, got {other:?}"),
    }
}

#[test]
fn merge_definitions_uses_barrel_resolver_for_non_carrier_targets() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let type_defs = vec![TypeLocation {
        path: "/home/user/index.ts".to_string(),
        start: 20,
        end: 27,
    }];
    let expected = Location {
        uri: file_path_to_uri("/home/user/Overlay.vue").unwrap(),
        range: Range {
            start: Position {
                line: 3,
                character: 2,
            },
            end: Position {
                line: 3,
                character: 9,
            },
        },
    };
    let resolver = |path: &str, start: u32, end: u32| {
        if path == "/home/user/index.ts" && start == 20 && end == 27 {
            Some(expected.clone())
        } else {
            None
        }
    };

    let result = merge_definitions_with_barrel_resolver(
        None,
        type_defs,
        "",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &test_doc_uri(),
        &carrier_exists,
        Some(&resolver),
        PositionEncodingKind::UTF16,
        &no_external_source,
    );

    match result {
        Some(GotoDefinitionResponse::Scalar(loc)) => assert_eq!(loc, expected),
        other => panic!("expected scalar resolved location, got {:?}", other),
    }
}

/// Regression: when verter resolves to a same-file import and TSGO resolves
/// to an external file (e.g., runtime-dom.d.ts), TSGO's cross-file result
/// must win — verter's same-file import is just an intermediate step.
#[test]
fn merge_definitions_tsgo_external_overrides_verter_same_file() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // Verter found the import statement (same file — uses SAME_FILE_URI sentinel)
    let verter_def = Some(GotoDefinitionResponse::Scalar(Location {
        uri: crate::features::definition::SAME_FILE_URI.clone(),
        range: Range {
            start: Position {
                line: 1,
                character: 0,
            },
            end: Position {
                line: 1,
                character: 20,
            },
        },
    }));

    // TSGO resolved to a real external .d.ts file (stands in for runtime-dom.d.ts).
    // `defineProps` sits on line 1, so the cross-file result resolves to line 1.
    let source = "export {}\nexport declare function defineProps(): void\n";
    let off = source.find("defineProps").unwrap() as u32;
    let (dts_path, read_source) = ext_source(".d.ts", source);
    let type_defs = vec![TypeLocation {
        path: dts_path.clone(),
        start: off,
        end: off + "defineProps".len() as u32,
    }];

    let result = merge_definitions(
        verter_def,
        type_defs,
        "",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &test_doc_uri(),
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &read_source,
    );
    assert!(result.is_some(), "should return TSGO's external definition");

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(loc) => {
            assert!(
                loc.uri.as_str().ends_with(".d.ts"),
                "should navigate to external .d.ts file, got: {}",
                loc.uri.as_str()
            );
            // The external result resolves to the real declaration line (not line 0).
            assert_eq!(
                loc.range.start.line, 1,
                "must resolve to the real symbol line"
            );
            // Negative: must NOT be the same-file sentinel URI
            assert!(
                !loc.uri
                    .as_str()
                    .contains(crate::features::definition::SAME_FILE_URI_STR),
                "must not return same-file sentinel when TSGO has external target"
            );
        }
        _ => panic!("Expected scalar definition for single external target"),
    }
}

/// @ai-generated — merge_definitions prefers verter when type_defs is empty
#[test]
fn merge_definitions_verter_preferred_when_no_type_defs() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

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

    let result = merge_definitions(
        verter_def,
        vec![],
        "",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &test_doc_uri(),
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );
    assert!(result.is_some());
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(loc) => {
            assert_eq!(loc.range.start.line, 5);
            assert_eq!(loc.range.start.character, 6);
        }
        _ => panic!("Expected scalar definition"),
    }
}

/// When verter provides a same-file definition (URI == document_uri) and
/// the type provider also returns results for the same .vue.tsx file,
/// verter should be preferred — its analysis spans are precise, while
/// the type provider's .vue.tsx byte offsets may fail position mapping.
#[test]
fn merge_definitions_prefers_verter_same_file_over_type_provider() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // Verter resolved to line 5 in the same file (sentinel already replaced)
    let verter_def = Some(GotoDefinitionResponse::Scalar(Location {
        uri: test_doc_uri(),
        range: Range {
            start: Position {
                line: 5,
                character: 6,
            },
            end: Position {
                line: 5,
                character: 12,
            },
        },
    }));

    // Type provider also returns a result for the same file's .vue.tsx
    // (position mapping will fail → would produce (0,0))
    let type_defs = vec![TypeLocation {
        path: "/test.vue.tsx".to_string(),
        start: 999,
        end: 1010,
    }];

    let result = merge_definitions(
        verter_def,
        type_defs,
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &test_doc_uri(),
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );
    assert!(
        result.is_some(),
        "should return verter's same-file definition"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(loc) => {
            // Positive: verter's precise position is preserved
            assert_eq!(loc.uri, test_doc_uri());
            assert_eq!(loc.range.start.line, 5);
            assert_eq!(loc.range.start.character, 6);
            // Negative: must NOT be (0,0) from failed type provider mapping
            assert_ne!(
                loc.range.start.line, 0,
                "must not be (0,0) from failed type provider mapping"
            );
        }
        _ => panic!("Expected scalar definition"),
    }
}

// ── Hover merge tests ──────────────────────────────────────────

// ── normalize_carrier_path tests ────────────────────────────────────

/// Predicate that always returns true — for tests where the .vue source is known to exist.
/// A source reader that resolves nothing — every external target fails closed (dropped).
/// Used by merge tests whose external fixtures only need to verify the carrier / fail-closed
/// branches; the readback-line:col behavior has its own boundary suite
/// (`tests/cross_file_navigation_ranges_fail_closed.rs`).
fn no_source(_: &str) -> Option<Arc<str>> {
    None
}

fn carrier_exists(_: &str) -> bool {
    true
}

/// Predicate that always returns false — simulates a real .vue.tsx file with no backing .vue.
fn carrier_missing(_: &str) -> bool {
    false
}

#[test]
fn normalize_carrier_path_strips_tsx() {
    assert_eq!(
        normalize_carrier_path("/src/App.vue.tsx", &carrier_exists),
        "/src/App.vue"
    );
}

#[test]
fn normalize_carrier_path_strips_dts() {
    assert_eq!(
        normalize_carrier_path("/node_modules/lib/Comp.vue.d.ts", &carrier_exists),
        "/node_modules/lib/Comp.vue"
    );
}

#[test]
fn normalize_carrier_path_strips_vue_ts() {
    assert_eq!(
        normalize_carrier_path("/src/App.vue.ts", &carrier_exists),
        "/src/App.vue"
    );
}

#[test]
fn normalize_carrier_path_strips_vue_jsx() {
    assert_eq!(
        normalize_carrier_path("/src/App.vue.jsx", &carrier_exists),
        "/src/App.vue"
    );
}

#[test]
fn normalize_carrier_path_strips_svelte_virtual_suffixes() {
    // Generalized to the carrier-extension set: a `.svelte` IDE/api/dts
    // virtual file normalizes back to the `.svelte` source.
    assert_eq!(
        normalize_carrier_path("/src/Comp.svelte.tsx", &carrier_exists),
        "/src/Comp.svelte"
    );
    assert_eq!(
        normalize_carrier_path("/src/Comp.svelte.ts", &carrier_exists),
        "/src/Comp.svelte"
    );
    assert_eq!(
        normalize_carrier_path("/node_modules/lib/C.svelte.d.ts", &carrier_exists),
        "/node_modules/lib/C.svelte"
    );
    assert!(is_carrier_ide_path("/src/Comp.svelte.tsx"));
    // A plain `.ts`/`.tsx` is NOT a carrier virtual file (negative).
    assert!(!is_carrier_ide_path("/src/plain.tsx"));
    assert_eq!(
        normalize_carrier_path("/src/plain.ts", &carrier_exists),
        "/src/plain.ts"
    );
}

#[test]
fn real_svelte_rune_module_is_not_a_carrier_virtual_file() {
    // Co-existence: `store.svelte.ts` with NO backing `store.svelte` is
    // a REAL first-class rune module — NOT the `{carrier}.ts` component API
    // virtual file. The existence guard disambiguates it from `Foo.svelte` +
    // `.ts` (the strip applies ONLY when `Foo.svelte` backs it); the rune
    // module's own provider surface is served from its own canonical path,
    // never normalized to a sibling `.svelte` component. This guard is what
    // the references / rename / code-action merges depend on to fail-closed
    // (drop) a rewritten path rather than emit a line-0 edit into the wrong file.
    assert_eq!(
        normalize_carrier_path("/src/store.svelte.ts", &carrier_missing),
        "/src/store.svelte.ts",
        "a real rune module's path must pass through unchanged (no backing source)"
    );
    // Contrast: a backed `Foo.svelte.ts` IS the component API virtual file and normalizes.
    assert_eq!(
        normalize_carrier_path("/src/Foo.svelte.ts", &carrier_exists),
        "/src/Foo.svelte"
    );
}

#[test]
fn normalize_carrier_path_passthrough_plain_dts() {
    // Non-.vue .d.ts files should NOT be stripped
    assert_eq!(
        normalize_carrier_path(
            "/node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts",
            &carrier_exists
        ),
        "/node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts"
    );
}

#[test]
fn normalize_carrier_path_passthrough_plain_ts() {
    // Non-.vue .ts files should NOT be stripped
    assert_eq!(
        normalize_carrier_path("/src/utils.ts", &carrier_exists),
        "/src/utils.ts"
    );
}

#[test]
fn normalize_carrier_path_skips_real_vue_tsx() {
    // A real .vue.tsx file on disk (no backing .vue source) must NOT be stripped
    assert_eq!(
        normalize_carrier_path("/src/App.vue.tsx", &carrier_missing),
        "/src/App.vue.tsx",
        "real .vue.tsx should be left unchanged when no .vue source exists"
    );
}

#[test]
fn normalize_carrier_path_skips_real_vue_ts() {
    assert_eq!(
        normalize_carrier_path("/src/App.vue.ts", &carrier_missing),
        "/src/App.vue.ts",
        "real .vue.ts should be left unchanged when no .vue source exists"
    );
}

#[test]
fn normalize_carrier_path_strips_virtual_vue_tsx() {
    // Virtual .vue.tsx with a backing .vue source SHOULD be stripped
    let exists_for_app = |path: &str| path == "/src/App.vue";
    assert_eq!(
        normalize_carrier_path("/src/App.vue.tsx", &exists_for_app),
        "/src/App.vue",
        "virtual .vue.tsx should strip to .vue when source exists"
    );
}

#[test]
fn normalize_carrier_path_dts_always_strips_regardless_of_predicate() {
    // .vue.d.ts from node_modules has no collision risk — always strip
    assert_eq!(
        normalize_carrier_path("/node_modules/lib/Comp.vue.d.ts", &carrier_missing),
        "/node_modules/lib/Comp.vue",
        ".vue.d.ts should always strip regardless of predicate"
    );
}

// ── .vue.d.ts definition tests ──────────────────────────────────

/// A `.vue.d.ts` definition target fails closed. Its byte offsets index the generated
/// declaration file, but the URI we would emit is the `.vue` source (path normalization
/// rewrites `.vue.d.ts` → `.vue`) and no in-context sourcemap bridges them. Rather than
/// manufacture a line-0 `Range` into the wrong file, the merge drops the location.
#[test]
fn merge_definitions_carrier_dts_fails_closed_no_line_zero() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    let type_defs = vec![TypeLocation {
        path: "/node_modules/my-lib/dist/Button.vue.d.ts".to_string(),
        start: 0,
        end: 10,
    }];

    let result = merge_definitions(
        None,
        type_defs,
        "",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &test_doc_uri(),
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );
    // No real range is available, so no location is produced — never a (0,0) default.
    assert!(
        result.is_none(),
        "must fail closed for a `.vue.d.ts` target, got: {result:?}"
    );
}

/// A `{carrier}.d.ts` reference is FAIL-CLOSED, not line-0'd.
///
/// Normalization rewrites `Button.vue.d.ts` → `Button.vue`, but the provider's byte offsets index
/// the generated declaration file and no in-context sourcemap bridges them onto the carrier
/// source. The references merge therefore DROPS the location (mirroring the definition merge's
/// "normalization rewrote the path → None" arm) rather than emitting a wrong line-0 range into the
/// `.vue` file. A real mapper for this carrier→declaration projection does not exist yet.
#[test]
fn merge_references_vue_dts_is_dropped_not_zeroed() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    let type_refs = vec![TypeLocation {
        path: "/node_modules/my-lib/dist/Button.vue.d.ts".to_string(),
        start: 0,
        end: 10,
    }];

    let result = merge_references(
        None,
        type_refs,
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
    );
    assert!(
        result.is_none(),
        "a {{carrier}}.d.ts ref whose offsets have no carrier sourcemap must be dropped, not \
         emitted at line 0: {result:?}"
    );
}

/// FIX 3 (merge boundary): a REAL on-disk `{carrier}.ts` (here `Child.vue.ts` alongside an existing
/// `Child.vue`) whose `is_carrier_api_path` SUFFIX predicate matches, but for which the
/// identity-gated `external_api_resolver` DECLINES (it is NOT the synced virtual API surface), is
/// edited IN PLACE as a normal file — its rename edit lands in `Child.vue.ts` at the REAL symbol
/// span, and NOTHING is mapped into `Child.vue`. This discriminates the suffix-only classification
/// that would have mapped the real file's offsets into the `.vue` and corrupted it.
#[test]
fn merge_rename_real_on_disk_carrier_ts_edits_in_place_never_maps_into_vue() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // A REAL, hand-written `Child.vue.ts` next to `Child.vue`. The renamed symbol sits on line 1.
    let real_ts = "// hand-written sidecar\nexport const childHelper = 1\n";
    let off = real_ts.find("childHelper").unwrap() as u32;
    let real_path = "/src/Child.vue.ts".to_string();
    let reader_path = real_path.clone();
    let real_content: Arc<str> = Arc::from(real_ts);
    let read_source = move |p: &str| (p == reader_path.as_str()).then(|| real_content.clone());

    // `Child.vue` exists → `is_carrier_api_path("/src/Child.vue.ts")` is true (suffix+exists).
    let carrier_source_exists = |p: &str| p == "/src/Child.vue";

    // The identity-gated API resolver DECLINES this path: it is not the synced virtual surface.
    let api_resolver = |_p: &str| ApiSurfaceResolution::NotVirtual;

    let type_locations = vec![RenameLocation {
        path: real_path.clone(),
        start: off,
        end: off + "childHelper".len() as u32,
    }];

    let result = merge_rename_locations(
        None,
        type_locations,
        "childHelperRenamed",
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        Some(&api_resolver),
        &carrier_source_exists,
        PositionEncodingKind::UTF16,
        &read_source,
    );

    let edit = result.expect("the real-file rename edit must be produced in place, not dropped");
    let changes = edit.changes.expect("changes map");

    // NOTHING is mapped into `Child.vue` — the corruption the suffix-only classifier would cause.
    let vue_uri = path_to_uri("/src/Child.vue").unwrap();
    assert!(
        !changes.contains_key(&vue_uri),
        "a real on-disk Child.vue.ts must NEVER produce an edit in Child.vue: {changes:?}"
    );

    // The edit lands in the REAL `Child.vue.ts` at the real symbol line (1), never line 0.
    let real_uri = path_to_uri(&real_path).unwrap();
    let edits = changes
        .get(&real_uri)
        .unwrap_or_else(|| panic!("rename must edit the real {real_path} in place"));
    assert_eq!(
        edits.len(),
        1,
        "exactly one in-place edit in the real .vue.ts"
    );
    assert_eq!(
        edits[0].range.start.line, 1,
        "real-file rename edit must resolve to the real symbol line, not line 0: {:?}",
        edits[0].range
    );
    assert_eq!(edits[0].new_text, "childHelperRenamed");
    assert_ne!(edits[0].range, Range::default());
}

/// A cross-file `{carrier}.ts` PUBLIC-API rename target (the common case: tsserver renames an
/// imported component's `defineProps` prop and reports the edit against `Child.vue.ts`, where the
/// prop type is lifted into the `$props` / `new(props?)` declaration) maps its API-surface byte
/// offsets back to the `.vue` source through the API surface's CodeTransform sourcemap (the
/// `external_api_resolver`) and is INCLUDED at the resolved carrier range.
///
/// This is THE root-cause regression for the dropped cross-file `.vue` prop rename: without the
/// API branch, `Child.vue.ts` fell through to the external branch, where `normalize_carrier_path`
/// rewrote it to `Child.vue` (≠ original) → the edit was dropped → the rename touched only the
/// queried file. The mapped prop sits on carrier line 1, so a faithful resolve lands on line 1 —
/// discriminating against both the drop (no edit) and a line-0 collapse.
#[test]
fn merge_rename_carrier_api_target_maps_via_api_sourcemap_and_is_included() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // The foreign component's `.vue` source: `foo` prop on line 1.
    let api_carrier_source =
        "<script setup lang=\"ts\">\ndefineProps<{ foo: string }>();\n</script>\n";
    // Its generated public-API surface: the prop type `{ foo: string }` is inlined; `foo` is the
    // renamed identifier. Model a minimal API surface whose `foo` is on provider line 0.
    let api_surface = "declare const Child: { new(props?: { foo: string }): {} }\n";
    let api_foo = api_surface.find("foo").unwrap() as u32; // provider byte offset of `foo`
    let carrier_foo_line = 1u32;
    let carrier_foo_col = api_carrier_source
        .lines()
        .nth(1)
        .unwrap()
        .find("foo")
        .unwrap() as u32; // carrier col of `foo` on line 1

    // API-surface sourcemap: provider `foo` position → carrier `foo` position.
    let (api_foo_line, api_foo_col) = {
        let before = &api_surface[..api_foo as usize];
        let line = before.matches('\n').count() as u32;
        let col = api_foo - before.rfind('\n').map(|i| i as u32 + 1).unwrap_or(0);
        (line, col)
    };
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content("Child.vue", api_carrier_source);
    builder.add_token(
        api_foo_line,
        api_foo_col,
        carrier_foo_line,
        carrier_foo_col,
        Some(source_id),
        None,
    );
    let api_json = builder.into_sourcemap().to_json_string();
    let api_mapper =
        ProviderPositionMapper::source_map(PositionMapper::from_json(&api_json).unwrap());
    let api_provider_li = LineIndex::new_utf16(api_surface);
    let api_carrier_li = LineIndex::new_utf16(api_carrier_source);

    // `is_carrier_api_path` requires `path_is_carrier(strip ".ts")` AND the carrier source to exist.
    let api_path = "/src/Child.vue.ts".to_string();
    let carrier_source_exists = |p: &str| p == "/src/Child.vue";

    let type_locations = vec![RenameLocation {
        path: api_path.clone(),
        start: api_foo,
        end: api_foo + 3,
    }];

    // The API resolver hands back the foreign API context for this `{carrier}.ts` path only.
    let api_resolver = |p: &str| -> ApiSurfaceResolution {
        if p == api_path {
            ApiSurfaceResolution::Vouched(ExternalIdeContext {
                tsx_line_index: api_provider_li.clone(),
                mapper: api_mapper.clone(),
                carrier_line_index: api_carrier_li.clone(),
                // UTF-16-negotiated session: the negotiated carrier index is the same
                // UTF-16 index, so the re-emission round-trip is the identity.
                carrier_negotiated_line_index: Some(api_carrier_li.clone()),
            })
        } else {
            ApiSurfaceResolution::NotVirtual
        }
    };

    // WITHOUT the API resolver: the `{carrier}.ts` target is dropped (fail-closed) — proving the
    // resolver is what bridges it, never the current-file `.tsx` mapper.
    let dropped = merge_rename_locations(
        None,
        type_locations.clone(),
        "fooRenamed",
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        None,
        &carrier_source_exists,
        PositionEncodingKind::UTF16,
        &no_source,
    );
    assert!(
        dropped.is_none(),
        "a carrier API target with no API resolver must be DROPPED, never line-0'd: {dropped:?}"
    );

    // WITH the API resolver: the edit is included at the mapped carrier range (line 1).
    let result = merge_rename_locations(
        None,
        type_locations,
        "fooRenamed",
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        Some(&api_resolver),
        &carrier_source_exists,
        PositionEncodingKind::UTF16,
        &no_source,
    );

    let edit = result.expect("carrier API rename edit must be produced, not dropped");
    let changes = edit.changes.expect("changes map");
    let uri = path_to_uri("/src/Child.vue").unwrap();
    let edits = changes
        .get(&uri)
        .unwrap_or_else(|| panic!("rename must map the API target to the .vue carrier source"));
    assert_eq!(
        edits.len(),
        1,
        "exactly one edit at the mapped carrier prop"
    );
    assert_eq!(
        edits[0].range.start.line, carrier_foo_line,
        "API-surface offset must map to the carrier prop line, not line 0: {:?}",
        edits[0].range
    );
    assert_eq!(edits[0].range.start.character, carrier_foo_col);
    assert_eq!(edits[0].new_text, "fooRenamed");
    assert_ne!(
        edits[0].range,
        Range::default(),
        "carrier API rename edit must never be the (0,0) line-0 placeholder"
    );
}

/// FIX 1 (encoding boundary): under a UTF-8-negotiated session, a carrier-API prop rename whose
/// carrier line begins with NON-ASCII text resolves to the CORRECT carrier range.
///
/// The API surface's `CodeTransform` source map indexes positions in UTF-16, while the LSP edit
/// range must be in the negotiated (UTF-8) encoding. When the carrier line has multibyte text
/// before the prop, the UTF-16 column (what the source map produces) DIFFERS from the UTF-8 column
/// (what the editor expects). The encoding-correct path runs the source-map lookup in UTF-16, then
/// re-emits the mapped range in UTF-8 via a byte-offset round-trip. This test asserts the returned
/// column equals the UTF-8 byte column of the prop — which is STRICTLY GREATER than its UTF-16
/// column here — so it FAILS against feeding negotiated columns into the UTF-16 map / returning the
/// UTF-16 column verbatim as a UTF-8 LSP position.
#[test]
fn merge_rename_carrier_api_target_utf8_session_nonascii_prefix_maps_correct_range() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // Carrier `.vue`: line 1 begins with a multibyte identifier (`café` — `é` is 2 bytes / 1 UTF-16
    // unit) BEFORE the renamed `foo`, so `foo`'s UTF-8 column != its UTF-16 column.
    let api_carrier_source =
        "<script setup lang=\"ts\">\nconst café = defineProps<{ foo: string }>();\n</script>\n";
    let line1 = api_carrier_source.lines().nth(1).unwrap();
    let foo_byte_in_line = line1.find("foo").unwrap() as u32;
    // UTF-8 column = byte offset within the line (line is ASCII except `é` = 2 bytes before `foo`).
    let want_utf8_col = foo_byte_in_line;
    // UTF-16 column = code units before `foo` (`é` counts as 1, vs 2 bytes), so strictly smaller.
    let want_utf16_col = line1[..foo_byte_in_line as usize]
        .chars()
        .map(|c| c.len_utf16() as u32)
        .sum::<u32>();
    assert!(
        want_utf8_col > want_utf16_col,
        "fixture precondition: the multibyte prefix must make the UTF-8 col ({want_utf8_col}) \
         exceed the UTF-16 col ({want_utf16_col})"
    );

    // API surface (provider side): `foo` is the renamed identifier, ASCII-only so its provider
    // byte offset is unambiguous.
    let api_surface = "declare const Child: { new(props?: { foo: string }): {} }\n";
    let api_foo = api_surface.find("foo").unwrap() as u32;

    // Source map: API `foo` (UTF-16) → carrier `foo` (UTF-16 col on line 1).
    let (api_foo_line, api_foo_col) = {
        let before = &api_surface[..api_foo as usize];
        let line = before.matches('\n').count() as u32;
        let col = api_foo - before.rfind('\n').map(|i| i as u32 + 1).unwrap_or(0);
        (line, col)
    };
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content("Child.vue", api_carrier_source);
    builder.add_token(
        api_foo_line,
        api_foo_col,
        1,
        want_utf16_col,
        Some(source_id),
        None,
    );
    let api_json = builder.into_sourcemap().to_json_string();
    let api_mapper =
        ProviderPositionMapper::source_map(PositionMapper::from_json(&api_json).unwrap());

    // The encoding contract: provider-surface + carrier indexes in UTF-16 (source-map space); the
    // negotiated carrier index in UTF-8 for the final re-emission.
    let api_provider_li = LineIndex::new_utf16(api_surface);
    let api_carrier_utf16_li = LineIndex::new_utf16(api_carrier_source);
    let api_carrier_utf8_li = LineIndex::new(api_carrier_source, PositionEncodingKind::UTF8);

    let api_path = "/src/Child.vue.ts".to_string();
    let carrier_source_exists = |p: &str| p == "/src/Child.vue";
    let type_locations = vec![RenameLocation {
        path: api_path.clone(),
        start: api_foo,
        end: api_foo + 3,
    }];

    let api_resolver = |p: &str| -> ApiSurfaceResolution {
        if p == api_path {
            ApiSurfaceResolution::Vouched(ExternalIdeContext {
                tsx_line_index: api_provider_li.clone(),
                mapper: api_mapper.clone(),
                carrier_line_index: api_carrier_utf16_li.clone(),
                carrier_negotiated_line_index: Some(api_carrier_utf8_li.clone()),
            })
        } else {
            ApiSurfaceResolution::NotVirtual
        }
    };

    let result = merge_rename_locations(
        None,
        type_locations,
        "fooRenamed",
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        Some(&api_resolver),
        &carrier_source_exists,
        PositionEncodingKind::UTF8, // ← UTF-8-negotiated session
        &no_source,
    );

    let edit = result.expect("carrier API rename edit must be produced under a UTF-8 session");
    let changes = edit.changes.expect("changes map");
    let uri = path_to_uri("/src/Child.vue").unwrap();
    let edits = changes
        .get(&uri)
        .unwrap_or_else(|| panic!("rename must map the API target to the .vue carrier source"));
    assert_eq!(edits.len(), 1);
    assert_eq!(
        edits[0].range.start.line, 1,
        "mapped to carrier prop line 1"
    );
    assert_eq!(
        edits[0].range.start.character, want_utf8_col,
        "the edit column must be the UTF-8 byte column ({want_utf8_col}), NOT the UTF-16 column \
         ({want_utf16_col}) — an encoding-mismatched mapping returns the wrong (in-bounds) range \
         and corrupts the .vue: {:?}",
        edits[0].range
    );
    assert_ne!(
        edits[0].range.start.character, want_utf16_col,
        "discriminator: the UTF-8 column must differ from the UTF-16 column for this fixture"
    );
    assert_eq!(edits[0].new_text, "fooRenamed");
}

/// A `{carrier}.d.ts` rename location is FAIL-CLOSED, not line-0'd — a line-0 rename edit would
/// corrupt the carrier file. Same reasoning as the references twin above.
#[test]
fn merge_rename_vue_dts_is_dropped_not_zeroed() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    let type_locations = vec![RenameLocation {
        path: "/node_modules/my-lib/dist/Button.vue.d.ts".to_string(),
        start: 0,
        end: 10,
    }];

    let result = merge_rename_locations(
        None,
        type_locations,
        "NewName",
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        None,
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_source,
    );
    assert!(
        result.is_none(),
        "a {{carrier}}.d.ts rename whose offsets have no carrier sourcemap must be dropped, not \
         emitted at line 0: {result:?}"
    );
}

/// FIX 2 (merge boundary): a carrier-API rename whose API surface is NOT YET SYNCED (the
/// identity-gated resolver DECLINES) and which has NO real on-disk backing file is DROPPED
/// (fail-closed) — never mapped through a fresh source map at a guessed range.
///
/// This models the staleness window: the resolver is the authority on whether the surface is the
/// currently-synced virtual surface; when it declines and the source reader resolves nothing, the
/// edit must be dropped rather than line-0'd into the `.vue`.
#[test]
fn merge_rename_carrier_api_unsynced_surface_no_backing_file_is_dropped() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // `Child.vue` exists → `is_carrier_api_path` matches the suffix; but the surface is NOT synced
    // (resolver declines as NotVirtual) and there is NO real `Child.vue.ts` on disk (reader returns
    // None).
    let carrier_source_exists = |p: &str| p == "/src/Child.vue";
    let api_resolver = |_p: &str| ApiSurfaceResolution::NotVirtual;

    let type_locations = vec![RenameLocation {
        path: "/src/Child.vue.ts".to_string(),
        start: 30,
        end: 33,
    }];

    let result = merge_rename_locations(
        None,
        type_locations,
        "fooRenamed",
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        Some(&api_resolver),
        &carrier_source_exists,
        PositionEncodingKind::UTF16,
        &no_source, // no real backing file
    );

    assert!(
        result.is_none(),
        "an unsynced carrier-API surface with no backing file must be DROPPED (fail closed), never \
         mapped at a guessed range: {result:?}"
    );
}

/// H1 (corruption): a captured-but-SUPERSEDED virtual `{carrier}.ts` surface (the resolver
/// returns `VirtualDrop` — it WAS a synced virtual surface, but its generation was retired or its
/// snapshot has no source map) MUST fail closed, even when a REAL on-disk file backs that EXACT
/// path. The provider's offsets index the VIRTUAL generated content, so applying them to the
/// same-named real file would corrupt it.
///
/// Discriminating: the pre-fix resolver returned a bare `Option<ExternalIdeContext>`, so a
/// superseded virtual surface returned `None` — INDISTINGUISHABLE from "not a virtual surface".
/// The merge then fell through to the real-file branch and edited the real file IN PLACE with the
/// virtual offsets. The 3-state `ApiSurfaceResolution::VirtualDrop` makes the superseded-virtual
/// case explicit so the merge drops it. This test FAILS against the bare-`Option` fall-through
/// (the real file gets a bogus edit) and PASSES once `VirtualDrop` short-circuits to `continue`.
#[test]
fn merge_rename_superseded_virtual_surface_with_real_backing_file_fails_closed() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // A REAL on-disk file at the EXACT virtual path. Its `foo` symbol sits at byte offset 30 — but
    // those bytes are NOT the provider's offsets; the provider's offsets came from the (now
    // superseded) VIRTUAL surface. If the merge edited this real file it would land at a wrong
    // span and corrupt it.
    let real_ts = "// a real same-named sidecar that must NOT be touched\nexport const foo = 1\n";
    let real_path = "/src/Child.vue.ts".to_string();
    let reader_path = real_path.clone();
    let real_content: Arc<str> = Arc::from(real_ts);
    let read_source = move |p: &str| (p == reader_path.as_str()).then(|| real_content.clone());

    // `Child.vue` exists → `is_carrier_api_path("/src/Child.vue.ts")` is true (suffix + exists), so
    // the merge CONSULTS the api resolver for this path (the suffix gate is unchanged).
    let carrier_source_exists = |p: &str| p == "/src/Child.vue";

    // The resolver classifies this path as a KNOWN virtual surface that can no longer be mapped
    // (generation re-check failed, or its snapshot carried no source map) → VirtualDrop.
    let api_resolver = |_p: &str| ApiSurfaceResolution::VirtualDrop;

    // The provider reports an offset against the (superseded) virtual surface.
    let type_locations = vec![RenameLocation {
        path: real_path.clone(),
        start: 36, // a virtual-surface offset; meaningless against the real file
        end: 39,
    }];

    let result = merge_rename_locations(
        None,
        type_locations,
        "fooRenamed",
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        Some(&api_resolver),
        &carrier_source_exists,
        PositionEncodingKind::UTF16,
        &read_source,
    );

    // FAIL CLOSED: nothing is edited. NOT the real `Child.vue.ts` (corruption the bare-Option
    // fall-through caused), NOT `Child.vue` (no source map vouched it).
    assert!(
        result.is_none(),
        "a superseded virtual carrier-API surface must DROP even with a real same-named backing \
         file — editing the real file with virtual offsets corrupts it: {result:?}"
    );
}

/// H1 companion: a genuinely `NotVirtual` path (the resolver returns `NotVirtual` — it was NEVER a
/// captured virtual surface) WITH a real on-disk backing file IS edited in place at the real
/// symbol span. This proves the 3-state outcome still routes real files correctly (the
/// `NotVirtual` arm preserves the existing real-file behavior, distinct from `VirtualDrop`).
///
/// Discriminating: together with the `VirtualDrop` test above, this pins that `NotVirtual` →
/// edit-in-place while `VirtualDrop` → drop, for the SAME path shape and the SAME real backing
/// file. A bare-`Option` resolver cannot express that split — both would be `None`.
#[test]
fn merge_rename_not_virtual_path_with_real_backing_file_edits_in_place() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // A REAL hand-written sidecar `Child.vue.ts`; the renamed symbol sits on line 1.
    let real_ts = "// hand-written sidecar\nexport const childHelper = 1\n";
    let off = real_ts.find("childHelper").unwrap() as u32;
    let real_path = "/src/Child.vue.ts".to_string();
    let reader_path = real_path.clone();
    let real_content: Arc<str> = Arc::from(real_ts);
    let read_source = move |p: &str| (p == reader_path.as_str()).then(|| real_content.clone());

    let carrier_source_exists = |p: &str| p == "/src/Child.vue";

    // The resolver classifies this path as NOT a virtual surface (it was never captured/synced).
    let api_resolver = |_p: &str| ApiSurfaceResolution::NotVirtual;

    let type_locations = vec![RenameLocation {
        path: real_path.clone(),
        start: off,
        end: off + "childHelper".len() as u32,
    }];

    let result = merge_rename_locations(
        None,
        type_locations,
        "childHelperRenamed",
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        Some(&api_resolver),
        &carrier_source_exists,
        PositionEncodingKind::UTF16,
        &read_source,
    );

    let edit = result.expect("a NotVirtual real-file rename edit must be produced in place");
    let changes = edit.changes.expect("changes map");

    // NOTHING is mapped into `Child.vue`.
    let vue_uri = path_to_uri("/src/Child.vue").unwrap();
    assert!(
        !changes.contains_key(&vue_uri),
        "a NotVirtual real Child.vue.ts must NEVER produce an edit in Child.vue: {changes:?}"
    );

    // The edit lands in the REAL `Child.vue.ts` at the real symbol line (1), never line 0.
    let real_uri = path_to_uri(&real_path).unwrap();
    let edits = changes
        .get(&real_uri)
        .unwrap_or_else(|| panic!("rename must edit the real {real_path} in place"));
    assert_eq!(
        edits.len(),
        1,
        "exactly one in-place edit in the real .vue.ts"
    );
    assert_eq!(
        edits[0].range.start.line, 1,
        "real-file rename edit must resolve to the real symbol line, not line 0: {:?}",
        edits[0].range
    );
    assert_eq!(edits[0].new_text, "childHelperRenamed");
    assert_ne!(edits[0].range, Range::default());
}

/// A2 end-to-end (store → classifier → merge): a path the STORE knows as a virtual
/// surface (tombstoned by an in-flight close) but ABSENT from the rename's in-flight
/// capture MUST route `VirtualDrop` and edit NEITHER the same-named real file NOR the
/// `.vue` — even though `close_dts` may have failed and tsserver is still live for the
/// virtual `{carrier}.ts`. The companion below proves a genuinely UNKNOWN path (never a
/// virtual surface) with the SAME real backing file DOES edit it in place (`NotVirtual`).
///
/// This drives the EXACT production rename closure (`classify_captured_api_surface`) over
/// a real `ProviderSurfaceStore`, not an injected resolution — so it discriminates the A2
/// fix end to end: pre-fix the resolver returned `NotVirtual` for every captured-miss, so
/// the tombstoned path fell through to the real-file branch and corrupted it. Post-fix the
/// store's tombstone routes it to `VirtualDrop`.
#[test]
fn merge_rename_store_known_virtual_absent_from_capture_routes_virtual_drop_end_to_end() {
    use crate::provider_surface_store::{
        classify_captured_api_surface, ProviderSurfaceKind, ProviderSurfaceStore, RecordSurface,
    };

    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    let vpath = "/src/Child.vue.ts".to_string();
    // A REAL same-named sidecar with a renameable symbol on line 1 — the file that would be
    // corrupted if a virtual-surface miss degraded to NotVirtual.
    let real_ts = "// real sidecar that must NOT be touched\nexport const foo = 1\n";
    let real_content: Arc<str> = Arc::from(real_ts);
    let reader_path = vpath.clone();
    let read_source = move |p: &str| (p == reader_path.as_str()).then(|| real_content.clone());
    let carrier_source_exists = |p: &str| p == "/src/Child.vue";

    // Store knows VPATH as a virtual surface, then RETIRES it (close started) — the tombstone
    // persists because the close is not finalized (simulating a failed/dropped close_dts).
    let store = ProviderSurfaceStore::new();
    store.record(RecordSurface {
        provider_path: vpath.clone(),
        kind: ProviderSurfaceKind::CarrierApi,
        source_canonical: "/src/Child.vue".to_string(),
        provider_content: Arc::from("declare const Child: {}\n"),
        source_map: None,
        carrier_source: Arc::from("<script setup>\n</script>\n"),
    });
    let _t = store.forget(&vpath);
    // Capture AFTER the retire → VPATH has no MAPPABLE snapshot (snapshot_for None), but it
    // is captured as KnownNonMappable (Closing at capture) so classify drops WITHOUT a live
    // re-consult of the store.
    let captured = store.capture_current_carrier_api_set();
    assert!(captured.snapshot_for(&vpath).is_none());
    assert!(store.is_known_virtual_surface(&vpath));

    // Classify reads ONLY the captured snapshot — no `store` arg (the third-TOCTOU fix).
    let api_resolver =
        |p: &str| classify_captured_api_surface(&captured, p, PositionEncodingKind::UTF16);

    let type_locations = vec![RenameLocation {
        path: vpath.clone(),
        start: 36, // a virtual-surface offset; meaningless against the real file
        end: 39,
    }];

    let result = merge_rename_locations(
        None,
        type_locations,
        "fooRenamed",
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        Some(&api_resolver),
        &carrier_source_exists,
        PositionEncodingKind::UTF16,
        &read_source,
    );

    assert!(
        result.is_none(),
        "a store-known virtual surface absent from the capture must DROP (fail closed) — \
         editing the same-named real file with virtual offsets corrupts it: {result:?}"
    );
}

/// A2 companion: the SAME path shape + the SAME real backing file, but the store does NOT
/// know the path as a virtual surface (never recorded). The classifier routes `NotVirtual`
/// and the real file IS edited in place — proving the tombstone, not the path shape, drives
/// the `VirtualDrop` vs `NotVirtual` split.
#[test]
fn merge_rename_store_unknown_path_with_real_backing_edits_in_place_end_to_end() {
    use crate::provider_surface_store::{classify_captured_api_surface, ProviderSurfaceStore};

    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    let vpath = "/src/Child.vue.ts".to_string();
    let real_ts = "// hand-written sidecar\nexport const foo = 1\n";
    let off = real_ts.find("foo").unwrap() as u32;
    let real_content: Arc<str> = Arc::from(real_ts);
    let reader_path = vpath.clone();
    let read_source = move |p: &str| (p == reader_path.as_str()).then(|| real_content.clone());
    let carrier_source_exists = |p: &str| p == "/src/Child.vue";

    // Empty store: VPATH is NOT a known virtual surface → absent from the capture → NotVirtual.
    let store = ProviderSurfaceStore::new();
    let captured = store.capture_current_carrier_api_set();
    assert!(!store.is_known_virtual_surface(&vpath));

    // Classify reads ONLY the captured snapshot — no `store` arg (the third-TOCTOU fix).
    let api_resolver =
        |p: &str| classify_captured_api_surface(&captured, p, PositionEncodingKind::UTF16);

    let type_locations = vec![RenameLocation {
        path: vpath.clone(),
        start: off,
        end: off + 3,
    }];

    let result = merge_rename_locations(
        None,
        type_locations,
        "fooRenamed",
        "/test.vue.tsx",
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        Some(&api_resolver),
        &carrier_source_exists,
        PositionEncodingKind::UTF16,
        &read_source,
    );

    let edit = result.expect("an unknown-path real-file rename edit must be produced in place");
    let changes = edit.changes.expect("changes map");
    let vue_uri = path_to_uri("/src/Child.vue").unwrap();
    assert!(
        !changes.contains_key(&vue_uri),
        "an unknown real Child.vue.ts must NEVER edit Child.vue: {changes:?}"
    );
    let real_uri = path_to_uri(&vpath).unwrap();
    let edits = changes
        .get(&real_uri)
        .unwrap_or_else(|| panic!("rename must edit the real {vpath} in place"));
    assert_eq!(
        edits[0].range.start.line, 1,
        "edit lands on the real symbol line"
    );
    assert_eq!(edits[0].new_text, "fooRenamed");
}

// ── Hover merge tests ──────────────────────────────────────────
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
    let carrier_li = LineIndex::new_utf16("");

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

    let result = merge_hover(verter, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
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
    let carrier_li = LineIndex::new_utf16("");

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

    let result = merge_hover(verter, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
    assert!(result.is_some());

    let text = match result.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };

    // Only TSGO type block, no "---" separator since verter had nothing extra
    assert_eq!(text, "```typescript\nconst x: string\n```");
}

// ── Bug 3: TSGO already-markdown hover tests ─────────────────

#[test]
fn merge_hover_tsgo_already_markdown_no_double_fence() {
    let (mapper, _, tsx_li) = make_mapper_and_indexes();
    let carrier_li = LineIndex::new_utf16("");

    let tsgo = Some(HoverInfo {
        range_start: None,
        range_end: None,
        contents: "```typescript\n(property) msg: string\n```\nThe message.".to_string(),
    });

    let result = merge_hover(None, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
    assert!(result.is_some());

    let text = match result.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };

    // Should start with the type signature in a code fence
    assert!(
        text.starts_with("```typescript\n(property) msg: string\n```"),
        "should start with original code fence: {text}"
    );
    // Documentation should appear OUTSIDE the code fence
    assert!(
        text.contains("The message."),
        "documentation should be present: {text}"
    );
    // Count code fence openings — should be exactly 1
    assert_eq!(
        text.matches("```typescript").count(),
        1,
        "should not double-fence: {text}"
    );
}

#[test]
fn merge_hover_tsgo_plain_text_gets_wrapped() {
    let (mapper, _, tsx_li) = make_mapper_and_indexes();
    let carrier_li = LineIndex::new_utf16("");

    let tsgo = Some(HoverInfo {
        range_start: None,
        range_end: None,
        contents: "(property) msg: string".to_string(),
    });

    let result = merge_hover(None, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
    assert!(result.is_some());

    let text = match result.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };

    assert_eq!(text, "```typescript\n(property) msg: string\n```");
}

#[test]
fn merge_hover_tsgo_with_jsdoc_newlines_preserved() {
    let (mapper, _, tsx_li) = make_mapper_and_indexes();
    let carrier_li = LineIndex::new_utf16("");

    let tsgo = Some(HoverInfo {
            range_start: None,
            range_end: None,
            contents: "```typescript\n(property) select: (action: Action) => true\n```\nEmitted when selected.\n当选择时触发。".to_string(),
        });

    let result = merge_hover(None, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
    assert!(result.is_some());

    let text = match result.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };

    assert!(
        text.contains("Emitted when selected."),
        "documentation should be preserved: {text}"
    );
    assert!(
        text.contains("当选择时触发。"),
        "CJK documentation should be preserved: {text}"
    );
    // Doc should be outside code fence
    assert_eq!(
        text.matches("```typescript").count(),
        1,
        "should not double-fence: {text}"
    );
}

#[test]
fn merge_hover_verter_and_tsgo_combined_markdown() {
    let (mapper, _, tsx_li) = make_mapper_and_indexes();
    let carrier_li = LineIndex::new_utf16("");

    let verter = Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: "```typescript\nconst count\n```\n*(reactive)*".to_string(),
        }),
        range: None,
    });
    let tsgo = Some(HoverInfo {
        range_start: None,
        range_end: None,
        contents: "```typescript\nconst count: Ref<number>\n```\nA counter.".to_string(),
    });

    let result = merge_hover(verter, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
    assert!(result.is_some());

    let text = match result.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };

    // TSGO signature should be present (not double-fenced)
    assert!(
        text.contains("const count: Ref<number>"),
        "should have TSGO signature: {text}"
    );
    assert!(
        text.contains("*(reactive)*"),
        "should have verter context: {text}"
    );
    // Only 1 typescript code fence
    assert_eq!(
        text.matches("```typescript").count(),
        1,
        "should not double-fence: {text}"
    );
}

#[test]
fn wrap_type_block_plain_text_with_blank_line_separator() {
    let (mapper, _, tsx_li) = make_mapper_and_indexes();
    let carrier_li = LineIndex::new_utf16("");

    // TSGO returns plain text with type and doc separated by blank line
    let tsgo = Some(HoverInfo {
        range_start: None,
        range_end: None,
        contents: "(property) GameItemProps.game: GameVo | ProfilePlayedVo\n\n游戏数据".to_string(),
    });

    let result = merge_hover(None, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
    let text = match result.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };

    // Type should be inside fence
    assert!(
        text.starts_with(
            "```typescript\n(property) GameItemProps.game: GameVo | ProfilePlayedVo\n```"
        ),
        "type should be in code fence: {text}"
    );
    // Doc should be outside fence
    assert!(
        text.contains("游戏数据"),
        "documentation should be preserved: {text}"
    );
    // Doc must not be inside the code fence
    let fence_end = text.find("\n```").unwrap();
    let doc_pos = text.find("游戏数据").unwrap();
    assert!(
        doc_pos > fence_end,
        "documentation should be outside the code fence: {text}"
    );
}

#[test]
fn wrap_type_block_plain_text_with_single_newline_separator() {
    let (mapper, _, tsx_li) = make_mapper_and_indexes();
    let carrier_li = LineIndex::new_utf16("");

    // TSGO returns plain text with type and doc separated by single newline
    let tsgo = Some(HoverInfo {
        range_start: None,
        range_end: None,
        contents: "(property) game: GameVo\nThe game data.".to_string(),
    });

    let result = merge_hover(None, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
    let text = match result.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };

    // Type should be inside fence
    assert!(
        text.starts_with("```typescript\n(property) game: GameVo\n```"),
        "type should be in code fence: {text}"
    );
    // Doc should be outside fence
    let fence_end = text.find("\n```").unwrap();
    let doc_pos = text.find("The game data.").unwrap();
    assert!(
        doc_pos > fence_end,
        "documentation should be outside the code fence: {text}"
    );
}

#[test]
fn wrap_type_block_plain_text_no_newline() {
    // When there's no newline separator, everything goes in the fence
    // (can't reliably split type from doc without a separator)
    let (mapper, _, tsx_li) = make_mapper_and_indexes();
    let carrier_li = LineIndex::new_utf16("");

    let tsgo = Some(HoverInfo {
        range_start: None,
        range_end: None,
        contents: "(property) msg: string".to_string(),
    });

    let result = merge_hover(None, tsgo, &mapper, &tsx_li, &carrier_li, None, None);
    let text = match result.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };

    assert_eq!(text, "```typescript\n(property) msg: string\n```");
}

#[test]
fn replace_kind_prefix_replaces_const_with_ref() {
    let input = "```typescript\n(const) const count: Ref<number>\n```";
    let result = replace_kind_prefix(input, "ref");
    assert_eq!(result, "```typescript\n(ref) const count: Ref<number>\n```");
    assert!(!result.contains("(const)"), "old prefix must be replaced");
}

#[test]
fn replace_kind_prefix_no_prefix_passthrough() {
    let input = "```typescript\nconst count: number\n```";
    let result = replace_kind_prefix(input, "ref");
    // No `(...)` prefix to replace, so content passes through unchanged
    assert_eq!(result, input);
}

#[test]
fn merge_hover_with_vue_kind_label() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = make_verter_hover("```typescript\nconst count\n```\n\n*(ref — needs `.value`)*");
    let type_hover = HoverInfo {
        contents: "```typescript\n(const) const count: Ref<number>\n```".to_string(),
        range_start: None,
        range_end: None,
    };

    let result = merge_hover(
        Some(verter),
        Some(type_hover),
        &mapper,
        &tsx_li,
        &carrier_li,
        Some("ref"),
        None,
    );
    let text = match result.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    assert!(
        text.contains("(ref) const count"),
        "kind prefix should be replaced with vue label: {text}"
    );
    assert!(
        !text.contains("(const)"),
        "generic kind prefix must be replaced: {text}"
    );
}

#[test]
fn merge_hover_rewrites_primary_label_from_typed_event_provenance() {
    // The `onCustom` → `@custom` rewrite is driven by the TYPED
    // `HoverSourceToken::EventDirective` provenance, never by reparsing the
    // rendered verter hover text.
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = make_verter_hover("`@custom`\n\nListens for the `custom` event.");
    let type_hover = HoverInfo {
        contents: "(property) onCustom: (payload: string) => void".to_string(),
        range_start: None,
        range_end: None,
    };

    let token = HoverSourceToken::EventDirective {
        vue_attr: "@custom".to_string(),
    };
    let result = merge_hover(
        Some(verter),
        Some(type_hover),
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        Some(&token),
    );
    let text = match result.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    let first_content_line = text.lines().nth(1).unwrap_or_default();
    assert!(
        first_content_line.contains("@custom"),
        "primary hover label should use Vue event syntax, got: {text}"
    );
    assert!(
        !first_content_line.contains("onCustom"),
        "primary hover label must not expose TSX on* naming, got: {text}"
    );
}

#[test]
fn merge_hover_does_not_rewrite_label_without_typed_provenance() {
    // Discriminating: even when the verter hover TEXT contains a backticked
    // `@custom` token, the merge layer must NOT rewrite the TypeProvider label
    // unless TYPED provenance is supplied. This proves the label rewrite is
    // driven only by typed provenance, never by reparsing the hover markdown.
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let verter = make_verter_hover("`@custom`\n\nSome descriptive context.");
    let type_hover = HoverInfo {
        contents: "(property) onCustom: (payload: string) => void".to_string(),
        range_start: None,
        range_end: None,
    };

    let result = merge_hover(
        Some(verter),
        Some(type_hover),
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        None,
    );
    let text = match result.unwrap().contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected markup"),
    };
    let first_content_line = text.lines().nth(1).unwrap_or_default();
    assert!(
        first_content_line.contains("onCustom"),
        "without typed provenance the generated label must be preserved, got: {text}"
    );
    assert!(
        !first_content_line.contains("@custom"),
        "no text-based rewrite may occur without typed provenance, got: {text}"
    );
}

/// Cross-file (foreign) carrier IDE definition resolution is fail-closed and exact.
///
/// When TSGO returns a carrier IDE target that is NOT the file being queried, only that
/// target's own sourcemap (via the external resolver) can map its byte offsets back to the
/// carrier source. Without a resolver the location is DROPPED — never collapsed to a line-0
/// range pointing into the wrong file (the bug this guards). With the resolver it maps to
/// the exact carrier range.
#[test]
fn merge_definitions_foreign_carrier_tsx_fails_closed_without_resolver_else_exact() {
    let (_mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // The file being queried (its in-context mapper) — distinct from the target, so the
    // target is genuinely FOREIGN and the current mapper must never be used for it.
    let current_tsx_path = "/src/components/Caller.vue.tsx";

    // Build the target file's own mapper: TSX 0:0 → Vue 1:0, TSX 0:16 → Vue 1:16.
    let target_carrier = "<script setup>\ndefineComponent({})\n</script>";
    let target_tsx = "defineComponent({});\n";
    let target_carrier_li = LineIndex::new_utf16(target_carrier);
    let target_tsx_li = LineIndex::new_utf16(target_tsx);

    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let sid = builder.set_source_and_content("Target.vue", target_carrier);
    builder.add_token(0, 0, 1, 0, Some(sid), None); // TSX 0:0 → Vue 1:0
    builder.add_token(0, 16, 1, 16, Some(sid), None); // TSX 0:16 → Vue 1:16
    let json = builder.into_sourcemap().to_json_string();
    let target_mapper =
        ProviderPositionMapper::source_map(PositionMapper::from_json(&json).unwrap());

    let type_defs = vec![TypeLocation {
        path: "/src/components/Target.vue.tsx".to_string(),
        start: 0,
        end: 16, // "defineComponent("
    }];

    // Without a resolver the foreign target has no usable sourcemap → fail closed. The
    // current file's mapper describes a DIFFERENT file and must NOT be reused, so the only
    // location is dropped and the merge returns the (empty) verter result.
    let result_no_resolver = merge_definitions(
        None,
        type_defs.clone(),
        current_tsx_path,
        &tsx_li,
        &_mapper,
        &carrier_li,
        None,
        &test_doc_uri(),
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );
    assert!(
            result_no_resolver.is_none(),
            "foreign carrier IDE target with no resolver must be DROPPED, never a line-0 range: {result_no_resolver:?}"
        );

    // With the resolver: the target's own mapper resolves the offsets to the exact range.
    let resolver = |ide_path: &str| -> Option<ExternalIdeContext> {
        if ide_path == "/src/components/Target.vue.tsx" {
            Some(ExternalIdeContext {
                tsx_line_index: target_tsx_li.clone(),
                mapper: target_mapper.clone(),
                carrier_line_index: target_carrier_li.clone(),
                carrier_negotiated_line_index: None,
            })
        } else {
            None
        }
    };

    let result_with_resolver = merge_definitions(
        None,
        type_defs,
        current_tsx_path,
        &tsx_li,
        &_mapper,
        &carrier_li,
        Some(&resolver),
        &test_doc_uri(),
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );
    match result_with_resolver {
        Some(GotoDefinitionResponse::Scalar(loc)) => {
            assert!(
                loc.uri.as_str().ends_with("Target.vue"),
                "should navigate to .vue: {}",
                loc.uri.as_str()
            );
            // Exact full range: "defineComponent(" at Vue (1,0)..(1,16) — both endpoints,
            // not just "line 1", and never the (0,0) default.
            assert_eq!(
                loc.range,
                Range {
                    start: Position {
                        line: 1,
                        character: 0,
                    },
                    end: Position {
                        line: 1,
                        character: 16,
                    },
                },
                "with resolver, expected exact Vue range (1,0)..(1,16), got: {:?}",
                loc.range
            );
            assert_ne!(
                loc.range,
                Range::default(),
                "with resolver, range must not be the (0,0) default"
            );
        }
        other => panic!("expected scalar definition, got: {other:?}"),
    }
}

// ── Definition deduplication and filtering tests ──────────────

#[test]
fn merge_definitions_deduplicates_identical_carrier_locations() {
    // Two identical carrier IDE spans for the file currently being queried map through the
    // in-context mapper to the same carrier range, so they are true duplicates and collapse
    // to a single location. (Distinct ranges in one file are kept; that is covered by the
    // same-file multi-definition test.)
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    let current_tsx_path = "/src/components/Dropdown.vue.tsx";
    let type_defs = vec![
        TypeLocation {
            path: current_tsx_path.to_string(),
            start: 6,
            end: 10,
        },
        TypeLocation {
            path: current_tsx_path.to_string(),
            start: 6,
            end: 10,
        },
    ];

    let result = merge_definitions(
        None,
        type_defs,
        current_tsx_path,
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &test_doc_uri(),
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &no_external_source,
    );
    match result {
        Some(GotoDefinitionResponse::Scalar(_)) => {
            // Deduplicated to a single location — correct
        }
        Some(GotoDefinitionResponse::Array(locs)) => {
            panic!(
                "should deduplicate to Scalar, got Array with {} locations",
                locs.len()
            );
        }
        other => panic!("expected Scalar, got {:?}", other),
    }
}

#[test]
fn merge_definitions_filters_vue_when_non_carrier_exists() {
    // Bug: when CTRL+CLICKing on an import from a library, both .d.mts (real def)
    // and .vue.tsx (consumer) are returned. Should filter out .vue targets.
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    // The real library definition lives in an on-disk `.d.mts` (its offsets index
    // its own source); the two `.vue.tsx` consumer spans normalize to `.vue`.
    let source = "export {}\nexport declare function onClickOutside(): void\n";
    let off = source.find("onClickOutside").unwrap() as u32;
    let (dmts_path, read_source) = ext_source(".d.mts", source);
    let type_defs = vec![
        TypeLocation {
            path: dmts_path.clone(),
            start: off,
            end: off + "onClickOutside".len() as u32,
        },
        TypeLocation {
            path: "/src/components/Dropdown.vue.tsx".to_string(),
            start: 0,
            end: 10,
        },
        TypeLocation {
            path: "/src/components/Drawer.vue.tsx".to_string(),
            start: 0,
            end: 10,
        },
    ];

    // Both foreign carrier IDE consumers map back to real carrier ranges through the
    // external resolver (their own sourcemaps), so the filter sees genuine carrier locations
    // to drop — not the old line-0 fallback that fail-closed resolution removed.
    let consumer_carrier = "<script setup>\nconst x = 1;\n</script>";
    let consumer_tsx = "const x = 1;\n";
    let consumer_carrier_li = LineIndex::new_utf16(consumer_carrier);
    let consumer_tsx_li = LineIndex::new_utf16(consumer_tsx);
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let sid = builder.set_source_and_content("Consumer.vue", consumer_carrier);
    builder.add_token(0, 0, 1, 0, Some(sid), None); // TSX 0:0 → Vue 1:0
    builder.add_token(0, 10, 1, 10, Some(sid), None); // TSX 0:10 → Vue 1:10
    let consumer_mapper = ProviderPositionMapper::source_map(
        PositionMapper::from_json(&builder.into_sourcemap().to_json_string()).unwrap(),
    );
    let resolver = |ide_path: &str| -> Option<ExternalIdeContext> {
        if is_carrier_ide_path(ide_path) {
            Some(ExternalIdeContext {
                tsx_line_index: consumer_tsx_li.clone(),
                mapper: consumer_mapper.clone(),
                carrier_line_index: consumer_carrier_li.clone(),
                carrier_negotiated_line_index: None,
            })
        } else {
            None
        }
    };

    let result = merge_definitions(
        None,
        type_defs,
        "",
        &tsx_li,
        &mapper,
        &carrier_li,
        Some(&resolver),
        &test_doc_uri(),
        &carrier_exists,
        PositionEncodingKind::UTF16,
        &read_source,
    );
    match result {
        Some(GotoDefinitionResponse::Scalar(loc)) => {
            // Should keep only the .d.mts definition
            assert!(
                !loc.uri.as_str().contains(".vue"),
                ".vue targets should be filtered out, got: {:?}",
                loc.uri
            );
        }
        Some(GotoDefinitionResponse::Array(locs)) => {
            for loc in &locs {
                assert!(
                    !loc.uri.as_str().contains(".vue"),
                    ".vue targets should be filtered out when .d.mts exists, got: {:?}",
                    loc.uri
                );
            }
        }
        None => panic!("expected some definitions"),
        _ => panic!("unexpected response type"),
    }
}

// ── JSX→Vue reverse transformation tests ──────────────────────

#[test]
fn test_jsx_event_to_vue_click() {
    assert_eq!(jsx_prop_to_vue_attr("onClick"), Some("@click".to_string()));
}

#[test]
fn test_jsx_event_to_vue_custom() {
    assert_eq!(
        jsx_prop_to_vue_attr("onCustomEvent"),
        Some("@custom-event".to_string())
    );
}

#[test]
fn test_jsx_event_to_vue_update_model() {
    assert_eq!(
        jsx_prop_to_vue_attr("onUpdate:modelValue"),
        Some("@update:model-value".to_string())
    );
}

#[test]
fn test_jsx_prop_camel_to_kebab() {
    assert_eq!(
        jsx_prop_to_vue_attr("modelValue"),
        Some("model-value".to_string())
    );
}

#[test]
fn test_jsx_data_attr_unchanged() {
    assert_eq!(
        jsx_prop_to_vue_attr("data-id"),
        None // Already kebab, no transformation needed
    );
}

#[test]
fn test_jsx_simple_attr_unchanged() {
    // Simple lowercase attrs like "class", "id", "key" — no transformation
    assert_eq!(jsx_prop_to_vue_attr("class"), None);
    assert_eq!(jsx_prop_to_vue_attr("id"), None);
    assert_eq!(jsx_prop_to_vue_attr("key"), None);
    assert_eq!(jsx_prop_to_vue_attr("ref"), None);
}

#[test]
fn test_jsx_tab_index_lowercase() {
    assert_eq!(
        jsx_prop_to_vue_attr("tabIndex"),
        Some("tab-index".to_string())
    );
}

#[test]
fn test_merge_completions_transforms_jsx_events() {
    // Create a TSGO completion result with an onClick item
    let type_result = CompletionResult {
        items: vec![
            Completion {
                label: "onClick".to_string(),
                kind: Some(CompletionKind::Property),
                detail: None,
                documentation: None,
                sort_text: None,
                insert_text: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            Completion {
                label: "modelValue".to_string(),
                kind: Some(CompletionKind::Property),
                detail: None,
                documentation: None,
                sort_text: None,
                insert_text: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
        ],
        is_incomplete: false,
    };

    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    let (items, _) = merge_completions(
        vec![],
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        "tsgo",
        true, // template_attr_context
    );

    // onClick should be transformed to @click
    assert!(
        items.iter().any(|i| i.label == "@click"),
        "onClick should be transformed to @click, got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(
        !items.iter().any(|i| i.label == "onClick"),
        "onClick should NOT remain"
    );

    // modelValue should be transformed to model-value
    assert!(
        items.iter().any(|i| i.label == "model-value"),
        "modelValue should be transformed to model-value"
    );
}

#[test]
fn merge_expression_context_does_not_transform_jsx() {
    // When template_attr_context=false (expression context like {{ props. }}),
    // JSX prop names should NOT be transformed to Vue syntax.
    let type_result = CompletionResult {
        items: vec![
            Completion {
                label: "onClick".to_string(),
                kind: Some(CompletionKind::Property),
                detail: None,
                documentation: None,
                sort_text: None,
                insert_text: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            Completion {
                label: "modelValue".to_string(),
                kind: Some(CompletionKind::Property),
                detail: None,
                documentation: None,
                sort_text: None,
                insert_text: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
            Completion {
                label: "title".to_string(),
                kind: Some(CompletionKind::Property),
                detail: None,
                documentation: None,
                sort_text: None,
                insert_text: None,
                edit_range_start: None,
                edit_range_end: None,
                text_edit_new_text: None,
                insert_text_format: None,
                commit_characters: None,
                filter_text: None,
                preselect: None,
                label_details: None,
                data: None,
            },
        ],
        is_incomplete: false,
    };

    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    let (items, _) = merge_completions(
        vec![],
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        "tsgo",
        false, // NOT in template attr context — expression context
    );

    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // POSITIVE: labels should remain as-is
    assert!(
        labels.contains(&"onClick"),
        "onClick should remain as-is, got: {labels:?}"
    );
    assert!(
        labels.contains(&"modelValue"),
        "modelValue should remain as-is, got: {labels:?}"
    );
    assert!(
        labels.contains(&"title"),
        "title should remain as-is, got: {labels:?}"
    );

    // NEGATIVE: no Vue-transformed labels
    assert!(
        !labels.iter().any(|l| l.starts_with('@')),
        "no @-prefixed items in expression context, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"model-value"),
        "no kebab-case transformation in expression context, got: {labels:?}"
    );
}

#[test]
fn merge_enriches_verter_kind_from_type_provider() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    // Verter item has VARIABLE kind
    let verter = vec![CompletionItem {
        label: "inc".to_string(),
        kind: Some(CompletionItemKind::VARIABLE),
        ..Default::default()
    }];
    // Type provider has FUNCTION kind for the same label
    let type_result = CompletionResult {
        items: vec![Completion {
            label: "inc".to_string(),
            kind: Some(CompletionKind::Function),
            detail: None,
            documentation: None,
            edit_range_start: None,
            edit_range_end: None,
            text_edit_new_text: None,
            insert_text: None,
            sort_text: None,
            insert_text_format: None,
            commit_characters: None,
            filter_text: None,
            preselect: None,
            label_details: None,
            data: None,
        }],
        is_incomplete: false,
    };

    let (result, _) = merge_completions(
        verter,
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        "tsgo",
        false,
    );
    assert_eq!(result.len(), 1, "duplicate should be deduped");
    assert_eq!(
        result[0].kind,
        Some(CompletionItemKind::FUNCTION),
        "verter item should be enriched with type provider's FUNCTION kind"
    );
}

#[test]
fn merge_does_not_enrich_with_text_kind() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    // Verter item has VARIABLE kind
    let verter = vec![CompletionItem {
        label: "msg".to_string(),
        kind: Some(CompletionItemKind::VARIABLE),
        ..Default::default()
    }];
    // Type provider has Text kind (fallback) for the same label
    let type_result = CompletionResult {
        items: vec![Completion {
            label: "msg".to_string(),
            kind: Some(CompletionKind::Text),
            detail: None,
            documentation: None,
            edit_range_start: None,
            edit_range_end: None,
            text_edit_new_text: None,
            insert_text: None,
            sort_text: None,
            insert_text_format: None,
            commit_characters: None,
            filter_text: None,
            preselect: None,
            label_details: None,
            data: None,
        }],
        is_incomplete: false,
    };

    let (result, _) = merge_completions(
        verter,
        type_result,
        &mapper,
        &tsx_li,
        &carrier_li,
        None,
        "tsgo",
        false,
    );
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].kind,
        Some(CompletionItemKind::VARIABLE),
        "Text kind from type provider should NOT override verter's VARIABLE kind"
    );
}
