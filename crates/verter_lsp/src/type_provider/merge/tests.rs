//! Behavior-preserving unit tests for the `type_provider::merge` module.
//!
//! `use super::*` pulls in every merge function/type re-exported from the
//! `merge` module root; the remaining imports bring in the protocol DTOs,
//! line-index/position-mapper helpers, and sourcemap builder the fixtures use.

#![allow(clippy::too_many_arguments)]

use std::sync::Arc;

use tower_lsp_server::ls_types::*;
use verter_span::TsPosition;

use super::definition::{
    is_carrier_api_or_dts_path, is_carrier_ide_path, normalize_carrier_path, path_to_uri,
};
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
        insert_text: None,
        sort_text: None,
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
    }];

    let result = merge_diagnostics(verter, types, &tsx_li, &mapper, &carrier_li);
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
        },
        TypeDiagnostic {
            message: "'msg' is deprecated.".to_string(),
            severity: TypeDiagnosticSeverity::Hint,
            start: 6,
            end: 9,
            code: Some("6385".to_string()),
            tags: vec![TypeDiagnosticTag::Deprecated],
        },
    ];

    let result = merge_diagnostics(vec![], types, &tsx_li, &mapper, &carrier_li);
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
    }];

    let result = merge_diagnostics(vec![], types, &tsx_li, &mapper, &carrier_li);
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
    }];

    let result = merge_diagnostics(vec![], types, &tsx_li, &mapper, &carrier_li);
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
    }];

    let result = merge_diagnostics(verter, types, &tsx_li, &mapper, &carrier_li);
    assert!(result.is_empty(), "unmapped diagnostics should be filtered");
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

    let result = merge_references(
        verter,
        type_refs,
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
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
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
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
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
    );
    assert!(result.is_some());
    assert_eq!(result.unwrap().len(), 1);
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

    let result = merge_code_actions(actions, &tsx_li, &mapper, &carrier_li, &carrier_exists);
    assert_eq!(result.len(), 1);
}

/// @ai-generated — Empty actions returns empty vec
#[test]
fn merge_code_actions_empty() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();
    let result = merge_code_actions(vec![], &tsx_li, &mapper, &carrier_li, &carrier_exists);
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
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
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
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
    );
    assert!(result.is_none());
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
    // `Comp.svelte.ts` is the api virtual file ONLY when `Comp.svelte`
    // EXISTS (disambiguation against a real `.svelte.ts` rune module).
    assert!(is_carrier_api_or_dts_path(
        "/src/Comp.svelte.ts",
        &carrier_exists
    ));
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
    // `.ts` (the component API virtual file exists ONLY when `Foo.svelte`
    // backs it); the rune module's own provider surface is served from its
    // own canonical path, never normalized to a sibling `.svelte` component.
    assert!(!is_carrier_api_or_dts_path(
        "/src/store.svelte.ts",
        &carrier_missing
    ));
    // And it is NOT normalized to a sibling `.svelte` (the strip is guarded).
    assert_eq!(
        normalize_carrier_path("/src/store.svelte.ts", &carrier_missing),
        "/src/store.svelte.ts"
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

/// .vue.d.ts references should map to .vue
#[test]
fn merge_references_vue_dts_maps_to_vue() {
    let (mapper, carrier_li, tsx_li) = make_mapper_and_indexes();

    let type_refs = vec![TypeLocation {
        path: "/node_modules/my-lib/dist/Button.vue.d.ts".to_string(),
        start: 0,
        end: 10,
    }];

    let result = merge_references(
        None,
        type_refs,
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
    );
    assert!(result.is_some());
    let locs = result.unwrap();
    assert_eq!(locs.len(), 1);
    assert!(
        locs[0].uri.as_str().ends_with("Button.vue"),
        "should reference .vue, got: {}",
        locs[0].uri.as_str()
    );
    assert!(
        !locs[0].uri.as_str().contains(".d.ts"),
        "URI must not contain .d.ts suffix"
    );
}

/// .vue.d.ts rename locations should map to .vue
#[test]
fn merge_rename_vue_dts_maps_to_vue() {
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
        &tsx_li,
        &mapper,
        &carrier_li,
        None,
        &carrier_exists,
    );
    assert!(result.is_some());
    let edit = result.unwrap();
    let changes = edit.changes.unwrap();
    let uris: Vec<String> = changes.keys().map(|u| u.as_str().to_string()).collect();
    assert!(
        uris.iter().any(|u| u.ends_with("Button.vue")),
        "should rename in .vue file, got: {:?}",
        uris
    );
    assert!(
        !uris.iter().any(|u| u.contains(".d.ts")),
        "URI must not contain .d.ts suffix"
    );
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
            insert_text: None,
            sort_text: None,
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
            insert_text: None,
            sort_text: None,
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
