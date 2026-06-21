//! Provider-neutral completion-resolve mapping for the tsserver family.
//!
//! These are PURE unit tests (no `verter_session` / LSP dependency), so they run
//! under the default `cargo test -p verter_type_runtime` gate. Integration tests
//! that depend on `verter_session` or LSP internals live in `verter_lsp`.
//!
//! They characterize the Issue #1 root-cause fixes:
//!   * `parse_tsserver_completion` PRESERVES the entry's resolve handle
//!     (`name`/`source`/`data`) instead of discarding it (`data: None`);
//!   * `completion_entry_details_to_resolve_result` maps a
//!     `completionEntryDetails` response's auto-import `codeActions` into
//!     same-file `ResolvedTextEdit`s.

use std::collections::HashMap;
use std::sync::Arc;

use crate::protocol::CompletionResolveData;
use crate::tsserver::ipc::{
    byte_offset_to_tsserver_pos, completion_entry_details_to_resolve_result,
    parse_tsserver_completion, stamp_tsserver_completion_offset,
};

/// A module-export (auto-import) completion entry from tsserver carries
/// `source` + `data`. The parser MUST preserve them as a `TsserverEntry`
/// resolve handle — discarding them (the historical `data: None`) is exactly
/// what made auto-import-on-accept impossible, because the LSP could never
/// re-issue `completionEntryDetails`.
#[test]
fn parse_tsserver_completion_preserves_external_module_resolve_handle() {
    let entry = serde_json::json!({
        "name": "computed",
        "kind": "function",
        "sortText": "5",
        "source": "vue",
        "hasAction": true,
        "data": {
            "exportName": "computed",
            "moduleSpecifier": "vue",
            "fileName": "/node_modules/vue/dist/vue.d.ts"
        }
    });

    let parsed = parse_tsserver_completion(&entry).expect("entry parses");

    // Negative assertion: the handle must NOT be dropped.
    assert!(
        parsed.data.is_some(),
        "an external-module entry must carry a resolve handle, not `data: None` \
         (the auto-import root cause)"
    );

    match parsed.data {
        Some(CompletionResolveData::TsserverEntry {
            name,
            source,
            data,
            offset,
        }) => {
            assert_eq!(name, "computed");
            assert_eq!(source.as_deref(), Some("vue"));
            assert_eq!(
                data.as_ref()
                    .and_then(|d| d.get("exportName"))
                    .and_then(|v| v.as_str()),
                Some("computed"),
                "the opaque resolve `data` blob must survive verbatim"
            );
            // Stamped by `get_completions`; the parser leaves it 0.
            assert_eq!(
                offset, 0,
                "parser leaves the offset for get_completions to stamp"
            );
        }
        other => panic!("expected a TsserverEntry resolve handle, got {other:?}"),
    }
}

/// A plain local completion (no `source`/`data`) still gets a `TsserverEntry`
/// handle keyed on its name — a local symbol resolve just yields no auto-import
/// edits, never a panic or a lost handle.
#[test]
fn parse_tsserver_completion_local_entry_has_name_only_handle() {
    let entry = serde_json::json!({ "name": "myLocal", "kind": "const", "sortText": "0" });
    let parsed = parse_tsserver_completion(&entry).expect("entry parses");

    match parsed.data {
        Some(CompletionResolveData::TsserverEntry {
            name, source, data, ..
        }) => {
            assert_eq!(name, "myLocal");
            assert!(source.is_none(), "a local entry has no module source");
            assert!(data.is_none(), "a local entry has no resolve data blob");
        }
        other => panic!("expected a TsserverEntry handle, got {other:?}"),
    }
}

/// CONTRACT: actionability is `source`/`data` only — `hasAction` is NOT a
/// factor. A tsserver entry can set `hasAction: true` with NO `source`/`data`
/// for a non-import action class (class-member snippet completions,
/// object-literal missing-comma insertion, type-only-alias wrappers). Those are
/// NOT auto-imports — `getCompletionEntryDetails` keys the auto-import lookup on
/// `(name, source, data)`, so a bare-`hasAction` entry has no module `source` to
/// resolve an import against. It must therefore parse to a NON-actionable handle
/// (no resolve envelope), exactly like a plain local symbol.
///
/// Discriminating: if `is_actionable` were widened to also honor `hasAction`
/// (e.g. `source.is_some() || data.is_some() || has_action`), this entry would
/// (wrongly) become actionable and this assertion would fail. It pins the
/// durable "source/data only" contract for the auto-import rail.
#[test]
fn parse_tsserver_completion_has_action_without_source_or_data_is_not_actionable() {
    // A class-member snippet / missing-comma-style entry: tsserver marks it
    // `hasAction: true` but it carries NEITHER a module `source` NOR a resolve
    // `data` blob (it is not an import).
    let entry = serde_json::json!({
        "name": "onClick",
        "kind": "method",
        "sortText": "11",
        "hasAction": true
    });
    let parsed = parse_tsserver_completion(&entry).expect("entry parses");

    match &parsed.data {
        Some(CompletionResolveData::TsserverEntry {
            name, source, data, ..
        }) => {
            assert_eq!(name, "onClick");
            assert!(
                source.is_none(),
                "a bare-`hasAction` entry carries no module source"
            );
            assert!(
                data.is_none(),
                "a bare-`hasAction` entry carries no resolve data blob"
            );
        }
        other => panic!("expected a TsserverEntry handle, got {other:?}"),
    }

    // The CONTRACT assertion: `hasAction` alone does NOT make the handle
    // actionable. Only `source`/`data` (the auto-import resolve key) does.
    assert!(
        !parsed.data.as_ref().unwrap().is_actionable(),
        "a `hasAction:true` entry with no `source`/`data` must NOT be actionable — \
         the auto-import resolve contract is `source`/`data` only, and routing this \
         non-import action through the import envelope would yield no edit"
    );
}

/// Positive control: a `hasAction: true` entry that DOES carry `source` (a real
/// auto-import) IS actionable — confirming the prior test isolates the
/// no-`source`/`data` case, not `hasAction` per se.
#[test]
fn parse_tsserver_completion_has_action_with_source_is_actionable() {
    let entry = serde_json::json!({
        "name": "computed",
        "kind": "function",
        "sortText": "5",
        "source": "vue",
        "hasAction": true
    });
    let parsed = parse_tsserver_completion(&entry).expect("entry parses");
    assert!(
        parsed.data.as_ref().unwrap().is_actionable(),
        "an auto-import entry (carries `source`) is actionable regardless of `hasAction`"
    );
}

/// `get_completions` stamps the completion-site offset onto the handle so
/// `completionEntryDetails` can be re-issued at the right position.
#[test]
fn stamp_offset_sets_tsserver_entry_offset() {
    let entry = serde_json::json!({ "name": "computed", "kind": "function", "source": "vue" });
    let parsed = parse_tsserver_completion(&entry).expect("entry parses");
    let stamped = stamp_tsserver_completion_offset(parsed, 1234);
    match stamped.data {
        Some(CompletionResolveData::TsserverEntry { offset, .. }) => {
            assert_eq!(
                offset, 1234,
                "the request offset must be stamped onto the handle"
            );
        }
        other => panic!("expected a TsserverEntry handle, got {other:?}"),
    }
}

/// H3 characterization — STALE-OFFSET FRAGILITY: the offset stamped onto a
/// `TsserverEntry` handle at completion-LIST time is re-converted to a tsserver
/// `(line, offset)` against the buffer the provider holds at RESOLVE time. If the
/// buffer changed between list and accept (text inserted before the offset), the
/// SAME stored byte offset now resolves to a DIFFERENT line/col.
///
/// This pins the documented limitation: `resolve_completion` re-anchors the
/// stored byte offset positionally, so an edit that shifts the generated artifact
/// drifts the resolve position. (In practice resolve fires immediately on accept
/// and a mis-keyed resolve fails closed — no edit — never a wrong import; the
/// version-stamped fix is a follow-up.)
///
/// Discriminating: a stable buffer maps the offset to the SAME position both
/// times, while a buffer with a leading line inserted maps it to a position one
/// line further down — proving the offset is NOT version-anchored.
#[test]
fn stamped_offset_drifts_when_buffer_changes_before_resolve() {
    // The completion was requested at byte 20 of the list-time buffer.
    let list_time = "const a = 1;\nconst b = 2;\n"; // byte 20 is on line 1 (0-based)
    let entry = serde_json::json!({ "name": "computed", "kind": "function", "source": "vue" });
    let parsed = parse_tsserver_completion(&entry).expect("entry parses");
    let stamped = stamp_tsserver_completion_offset(parsed, 20);
    let CompletionResolveData::TsserverEntry { offset, .. } = stamped.data.unwrap() else {
        panic!("expected a TsserverEntry handle");
    };
    assert_eq!(offset, 20);

    // Resolve against the SAME buffer: byte 20 maps to the position it was
    // captured at (1-based line 2).
    let (line_stable, _) = byte_offset_to_tsserver_pos(list_time, offset);

    // Resolve against a buffer that gained a leading line between list and accept:
    // the SAME byte 20 now sits one line EARLIER in the document text, so the
    // re-converted position differs — the handle is not version-anchored.
    let edited = "// inserted header line\nconst a = 1;\nconst b = 2;\n";
    let (line_drifted, _) = byte_offset_to_tsserver_pos(edited, offset);

    assert_ne!(
        line_stable, line_drifted,
        "the stored byte offset re-converts to a DIFFERENT line against a changed \
         buffer — the documented stale-offset fragility (H3). resolve fails closed \
         on a drift, never a wrong import."
    );
}

/// `completionEntryDetails` for an auto-importable entry returns
/// `codeActions[].changes[].textChanges` — the import insertion. The shared
/// mapper MUST turn the SAME-FILE text changes into `ResolvedTextEdit`s
/// (generated-file byte offsets), which is what the historical trait-default
/// `Ok(None)` never produced.
#[test]
fn entry_details_map_same_file_auto_import_to_resolved_edits() {
    let generated = "/project/src/App.vue.tsx";
    // A faithful tsserver `completionEntryDetails` detail object for `computed`:
    // a top-of-file import insertion (line 1, offset 1 → byte 0) into the SAME
    // generated file.
    let detail = serde_json::json!({
        "name": "computed",
        "kind": "function",
        "displayParts": [
            { "text": "function", "kind": "keyword" },
            { "text": " ", "kind": "space" },
            { "text": "computed", "kind": "functionName" }
        ],
        "codeActions": [
            {
                "description": "Add import from \"vue\"",
                "changes": [
                    {
                        "fileName": generated,
                        "textChanges": [
                            {
                                "start": { "line": 1, "offset": 1 },
                                "end": { "line": 1, "offset": 1 },
                                "newText": "import { computed } from \"vue\";\n"
                            }
                        ]
                    }
                ]
            }
        ]
    });

    let mut cache = HashMap::new();
    // Generated content: the import lands at byte 0 (start of file).
    cache.insert(
        generated.to_string(),
        Arc::from("const x = 1;\nexport default {};\n"),
    );

    let result = completion_entry_details_to_resolve_result(&detail, generated, &cache)
        .expect("an auto-import entry detail yields a resolve result");

    assert_eq!(
        result.additional_text_edits.len(),
        1,
        "the same-file import insertion must become exactly one ResolvedTextEdit"
    );
    let edit = &result.additional_text_edits[0];
    assert_eq!(edit.new_text, "import { computed } from \"vue\";\n");
    assert_eq!(edit.start, 0, "line 1/offset 1 maps to byte 0");
    assert_eq!(edit.end, 0, "a zero-width insertion");
    // The detail/display is also surfaced.
    assert_eq!(
        result.detail.as_deref(),
        Some("function computed"),
        "displayParts must be folded into the resolved detail"
    );
}

/// Cross-file edits (an import added to a DIFFERENT module than the generated
/// file) are dropped by the mapper: the LSP carrier re-anchor owns the
/// generated-TSX → `.vue` mapping, so only same-file edits become resolved
/// edits. A detail whose ONLY edits are cross-file yields `None`.
#[test]
fn entry_details_drop_cross_file_edits() {
    let generated = "/project/src/App.vue.tsx";
    let detail = serde_json::json!({
        "name": "Foo",
        "codeActions": [
            {
                "description": "Add import",
                "changes": [
                    {
                        "fileName": "/project/src/other.ts",
                        "textChanges": [
                            {
                                "start": { "line": 1, "offset": 1 },
                                "end": { "line": 1, "offset": 1 },
                                "newText": "import { Foo } from './foo';\n"
                            }
                        ]
                    }
                ]
            }
        ]
    });
    let cache = HashMap::new();

    let result = completion_entry_details_to_resolve_result(&detail, generated, &cache);
    assert!(
        result.is_none(),
        "a detail with only cross-file edits and no detail/docs yields no resolve result"
    );
}
