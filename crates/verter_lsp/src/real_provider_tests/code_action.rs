//! Real-provider code-action round-trip tests (tsserver + TSGO).
//!
//! ISSUE-8: pressing CTRL+. on an unused `<script setup>` binding must offer
//! TypeScript's "Remove unused declaration" (and "Remove all unused
//! declarations") quick-fixes, with the edits applied to the `.vue` / `.svelte`
//! SOURCE — across the shared LSP, for both Vue and Svelte carriers.
//!
//! These drive the FULL carrier path: open a framework SFC whose `<script setup>`
//! has a genuinely-unused `const foo`, then issue `textDocument/codeAction` at the
//! decl range with the published TS6133 in `context.diagnostics`, and assert the
//! returned action's workspace edit DELETES `const foo` from the SFC source (the
//! carrier map-back), never the generated TSX.
//!
//! Provider coverage:
//! - **tsserver / extension-family**: TypeScript implements the `unusedIdentifier`
//!   codefix, so the remove-unused action is the BINDING assertion. Its
//!   `getCombinedCodeFix` "Delete all unused declarations" companion is tagged with
//!   a typed `fixId` (`unusedIdentifier_delete`) only when the file carries TWO OR
//!   MORE unused-identifier diagnostics: TypeScript's `removeFixIdIfFixAllUnavailable`
//!   gate strips the `fixId`/`fixAllDescription` from a single-unused-decl fix
//!   (counting the whole-file unused family — 6133, 6196, 6138, 6192, …). This is
//!   TS's stable intentional behavior, NOT a version limitation. So each fixture
//!   here declares TWO unused bindings, which makes tsserver attach the `fixId`.
//!   - **Vue** then surfaces the combined remove-all companion for real: the
//!     generated template-binding TSX carries no overloaded `declare` decls, so the
//!     provider's `getCombinedCodeFix` follow-up succeeds and maps the combined
//!     deletion back to the `.vue` source — asserted as a REAL end-to-end gate.
//!   - **Svelte** gets the `fixId` too, but the provider's `getCombinedCodeFix`
//!     follow-up THROWS inside `typescript@6.0.3` (`Debug Failure. False expression:
//!     Changes overlap`) because the generated Svelte prelude declares the runes
//!     (`$state`, `$derived`, …) as OVERLOADED `declare function` groups and TS's
//!     whole-file unused fix-all emits overlapping deletion ranges for those overload
//!     signatures. The provider fails closed on the throw, so the Svelte remove-all
//!     companion cannot surface today (an upstream TS `getCombinedCodeFix`
//!     limitation, NOT a count-gate or version issue) — it stays a fail-loud CANARY
//!     (tracked follow-up: emit non-overloaded rune declares so the prelude no longer
//!     poisons the whole-file fix-all). The single remove-unused deletion IS a real
//!     assert for both carriers.
//! - **TSGO**: typescript-go has NOT yet ported the unused-identifier codefix
//!   (its registered quickfix providers cover add-missing-import / isolated
//!   declarations / class-implements only — verified against
//!   `internal/ls/codeactions.go`). The TSGO variant therefore asserts the wire
//!   is correctly populated (the request now carries an integer-coded
//!   `context.diagnostics` instead of an empty array) and treats the absent
//!   remove-unused fix as a CANARY: when TSGO ports the codefix, the canary flips
//!   and the assertion is promoted. No further LSP change is required then.
//!
//! Vacuous-skip aware: the generated test returns early when the backend binary
//! is unavailable (no `node_modules`); under require-mode the assertions are
//! fail-closed.

use tower_lsp_server::ls_types::{
    CodeActionContext, CodeActionOrCommand, CodeActionParams, CodeActionResponse, Diagnostic,
    NumberOrString, PartialResultParams, Position, Range, TextDocumentIdentifier, Uri,
    WorkDoneProgressParams,
};
use tower_lsp_server::LanguageServer;

use crate::test_harness::{
    canary_assert_known_limitation, real_provider_test, RealProviderTestSession,
};

/// Find the `[line, character]` carrier position of `needle` (+ `delta`) in an
/// open document.
fn carrier_position(
    session: &RealProviderTestSession,
    uri: &Uri,
    needle: &str,
    delta: usize,
) -> Position {
    let doc = session
        .server()
        .test_documents()
        .get(uri)
        .expect("document should be open");
    let offset = doc
        .source
        .find(needle)
        .unwrap_or_else(|| panic!("needle `{needle}` should exist in document"))
        + delta;
    doc.line_index
        .offset_to_position(offset as u32)
        .expect("valid position")
}

/// Issue `textDocument/codeAction` over `range` with `diagnostics` as the
/// editor-sent `context.diagnostics`.
async fn code_action(
    session: &RealProviderTestSession,
    uri: &Uri,
    range: Range,
    diagnostics: Vec<Diagnostic>,
) -> Option<CodeActionResponse> {
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range,
        context: CodeActionContext {
            diagnostics,
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    match session.server().code_action(params).await {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!("code_action error: {e}");
            None
        }
    }
}

/// A published TS6133 diagnostic over `range` (the form the editor sends on
/// CTRL+. over a faded unused decl: `code: String("6133")`).
fn ts6133(range: Range, name: &str) -> Diagnostic {
    Diagnostic {
        range,
        code: Some(NumberOrString::String("6133".to_string())),
        source: Some("ts".to_string()),
        message: format!("'{name}' is declared but its value is never read."),
        ..Default::default()
    }
}

/// A published TS2304 diagnostic over `range` (the "Cannot find name" the editor
/// sends on CTRL+. over an unresolved identifier — the precondition for the
/// `addMissingImport` quick-fix: `code: String("2304")`).
fn ts2304(range: Range, name: &str) -> Diagnostic {
    Diagnostic {
        range,
        code: Some(NumberOrString::String("2304".to_string())),
        source: Some("ts".to_string()),
        message: format!("Cannot find name '{name}'."),
        ..Default::default()
    }
}

/// Position `a <= b` in (line, character) lexicographic order.
fn pos_le(a: Position, b: Position) -> bool {
    (a.line, a.character) <= (b.line, b.character)
}

/// Does any returned action delete the decl from the carrier `uri` SOURCE,
/// covering the whole `decl_ident` identifier span?
///
/// Tightened (F4): the edit must target the `.vue` / `.svelte` carrier URI itself
/// — never a generated `.tsx` — AND its deletion range must EQUAL or strictly
/// CONTAIN the `decl_ident` span at BOTH endpoints (not merely overlap the line).
/// A same-line-but-wrong span, or an edit still keyed to the `.tsx`, now fails.
fn has_remove_unused_deletion(actions: &CodeActionResponse, uri: &Uri, decl_ident: Range) -> bool {
    // The carrier URI must be the SFC source, not a generated TSX sidecar.
    assert!(
        !uri.as_str().ends_with(".tsx"),
        "the carrier uri under test must be the .vue/.svelte source, not a .tsx: {uri:?}"
    );
    actions.iter().any(|a| {
        let CodeActionOrCommand::CodeAction(ca) = a else {
            return false;
        };
        let Some(edit) = ca.edit.as_ref() else {
            return false;
        };
        let Some(changes) = edit.changes.as_ref() else {
            return false;
        };
        changes.iter().any(|(edit_uri, edits)| {
            // The deletion must be keyed by the carrier source URI (never a .tsx).
            edit_uri == uri
                && !edit_uri.as_str().ends_with(".tsx")
                && edits.iter().any(|e| {
                    // A deletion (empty new_text) whose range EQUALS or CONTAINS
                    // the identifier span at both endpoints.
                    e.new_text.is_empty()
                        && pos_le(e.range.start, decl_ident.start)
                        && pos_le(decl_ident.end, e.range.end)
                })
        })
    })
}

/// Does any returned action carry the combined "Delete all unused declarations"
/// fix-all companion that ALSO maps its deletion(s) back to the carrier source?
///
/// Two gates, mirroring `has_remove_unused_deletion`:
/// 1. the action title contains both "all" and "unused" (the combined
///    fix-all companion, titled verbatim from `fixAllDescription`); and
/// 2. it carries a workspace edit with at least one DELETION keyed to the
///    carrier `.vue` / `.svelte` `uri` (never a generated `.tsx`) whose range
///    covers the FIRST unused decl's identifier span (`first_decl_ident`).
///
/// This proves the combined fix's edits map back to the SFC source too — not
/// merely that a titled action came back.
fn has_remove_all_unused(actions: &CodeActionResponse, uri: &Uri, first_decl_ident: Range) -> bool {
    // The carrier URI under test must be the SFC source, not a generated TSX.
    assert!(
        !uri.as_str().ends_with(".tsx"),
        "the carrier uri under test must be the .vue/.svelte source, not a .tsx: {uri:?}"
    );
    actions.iter().any(|a| {
        let CodeActionOrCommand::CodeAction(ca) = a else {
            return false;
        };
        let title = ca.title.to_lowercase();
        if !(title.contains("all") && title.contains("unused")) {
            return false;
        }
        let Some(edit) = ca.edit.as_ref() else {
            return false;
        };
        let Some(changes) = edit.changes.as_ref() else {
            return false;
        };
        changes.iter().any(|(edit_uri, edits)| {
            // The combined deletion must be keyed by the carrier source URI
            // (never a .tsx) and cover the first unused decl's identifier span.
            edit_uri == uri
                && !edit_uri.as_str().ends_with(".tsx")
                && edits.iter().any(|e| {
                    e.new_text.is_empty()
                        && pos_le(e.range.start, first_decl_ident.start)
                        && pos_le(first_decl_ident.end, e.range.end)
                })
        })
    })
}

// ---------------------------------------------------------------------------
// Vue: remove-unused quick-fix maps back to the .vue source
// ---------------------------------------------------------------------------

real_provider_test!(
    vue_unused_script_setup_remove_declaration_maps_to_source,
    fixture = "single-project",
    async fn run(session) {
        // `unusedVue` AND `alsoUnusedVue` are referenced in neither template nor
        // script, so TWO TS6133 diagnostics fire (one per decl) and CTRL+. lands on
        // `unusedVue`. Two unused decls are required for tsserver to attach the
        // `fixId` that drives the combined "Delete all unused declarations" fix-all.
        let content = "\
<script setup lang=\"ts\">
const unusedVue = 1
const alsoUnusedVue = 3
const usedVue = 2
</script>
<template>
  <div>{{ usedVue }}</div>
</template>
";
        let uri = session
            .open_virtual("src/UnusedQuickFixCase.vue", content)
            .await;
        session.server().test_ensure_synced(&uri).await;

        let decl_start = carrier_position(session, &uri, "unusedVue", 0);
        let decl_end = carrier_position(session, &uri, "unusedVue", "unusedVue".len());
        let range = Range {
            start: decl_start,
            end: decl_end,
        };

        let actions = code_action(session, &uri, range, vec![ts6133(range, "unusedVue")]).await;

        // tgo FIRST, tolerant of a None/empty result: typescript-go has not ported
        // the unused-identifier codefix, so it returns no remove-unused action (an
        // empty/None response) even with the wire fully populated. This branch MUST
        // run before the empty-result skip, or under `VERTER_REQUIRE_TGO=1` the skip
        // path panics before the canary is ever reached.
        if session.is_tsgo() {
            let has = actions
                .as_ref()
                .is_some_and(|a| has_remove_unused_deletion(a, &uri, range));
            canary_assert_known_limitation!(
                !has,
                "TSGO has not ported the TS6133 unused-identifier codefix; the \
                 remove-unused quick-fix is unavailable on the tsgo backend until \
                 it does (the LSP wiring is forward-ready)."
            );
            return;
        }

        let Some(actions) = actions else {
            if session.allow_empty_result_skip("no code actions for unused .vue binding") {
                return;
            }
            panic!("expected code actions for the unused .vue binding");
        };

        // tsserver-family: the remove-unused deletion must come back, mapped to
        // the .vue SOURCE (never the generated TSX). This is the BINDING assertion.
        assert!(
            has_remove_unused_deletion(&actions, &uri, range),
            "the remove-unused quick-fix must delete `const unusedVue` from the \
             .vue source; got {actions:?}"
        );

        // With TWO unused decls in the file, tsserver tags the unused-identifier fix
        // with the typed `fixId` ("unusedIdentifier_delete") + `fixAllDescription`
        // ("Delete all unused declarations"), so the provider's `getCombinedCodeFix`
        // follow-up surfaces the combined remove-all companion. Assert it comes back
        // AND maps its deletion to the .vue source — proving the full carrier-mapped
        // fix-all end-to-end against the real tsserver.
        assert!(
            has_remove_all_unused(&actions, &uri, range),
            "the combined `Delete all unused declarations` fix-all companion must be \
             surfaced (and map back to the .vue source) when the file has multiple \
             unused declarations; got {actions:?}"
        );
    }
);

// ---------------------------------------------------------------------------
// Svelte: remove-unused quick-fix maps back to the .svelte source
// ---------------------------------------------------------------------------

real_provider_test!(
    svelte_unused_script_remove_declaration_maps_to_source,
    fixture = "single-project",
    async fn run(session) {
        // `unusedSvelte` AND `alsoUnusedSvelte` are both unused, so TWO TS6133
        // diagnostics fire — the count tsserver requires before it attaches the
        // `fixId` that drives the combined "Delete all unused declarations" fix-all.
        // CTRL+. lands on the first decl, `unusedSvelte`.
        let content = "\
<script lang=\"ts\">
const unusedSvelte = 1
const alsoUnusedSvelte = 3
const usedSvelte = 2
</script>
<div>{usedSvelte}</div>
";
        let uri = session
            .open_virtual("src/UnusedQuickFixCase.svelte", content)
            .await;
        session.server().test_ensure_synced(&uri).await;

        let decl_start = carrier_position(session, &uri, "unusedSvelte", 0);
        let decl_end = carrier_position(session, &uri, "unusedSvelte", "unusedSvelte".len());
        let range = Range {
            start: decl_start,
            end: decl_end,
        };

        let actions = code_action(session, &uri, range, vec![ts6133(range, "unusedSvelte")]).await;

        // tgo FIRST, tolerant of a None/empty result (see the Vue test): the canary
        // must run before the empty-result skip so require-mode does not panic first.
        if session.is_tsgo() {
            let has = actions
                .as_ref()
                .is_some_and(|a| has_remove_unused_deletion(a, &uri, range));
            canary_assert_known_limitation!(
                !has,
                "TSGO has not ported the TS6133 unused-identifier codefix; the \
                 remove-unused quick-fix is unavailable on the tsgo backend for \
                 the .svelte carrier too until it does."
            );
            return;
        }

        let Some(actions) = actions else {
            if session.allow_empty_result_skip("no code actions for unused .svelte binding") {
                return;
            }
            panic!("expected code actions for the unused .svelte binding");
        };

        assert!(
            has_remove_unused_deletion(&actions, &uri, range),
            "the remove-unused quick-fix must delete `const unusedSvelte` from the \
             .svelte source; got {actions:?}"
        );

        // The combined remove-all companion is provider-side and framework-agnostic,
        // and tsserver DOES tag the Svelte unused-decl fix with the `fixId`
        // ("unusedIdentifier_delete") here — the file has multiple unused decls, so
        // the count gate is open exactly as in the Vue case. BUT the provider's
        // `getCombinedCodeFix("unusedIdentifier_delete")` follow-up THROWS inside
        // `typescript@6.0.3` for the .svelte carrier — `Debug Failure. False
        // expression: Changes overlap` — because the generated Svelte prelude
        // declares the runes (`$state`, `$derived`, `$props`, …) as OVERLOADED
        // `declare function` groups, and TS's whole-file unused-identifier fix-all
        // emits overlapping/identical deletion ranges for those overload signatures.
        // (Isolated to a minimal repro: two overloaded `declare function $state`
        // signatures THROW; a single signature SUCCEEDS — so it is the prelude's
        // overloaded declares, not the user fixture.) The provider call fails closed
        // on the throw, so no combined action is surfaced for Svelte today. This is
        // an upstream TypeScript `getCombinedCodeFix` limitation interacting with the
        // Svelte rune prelude — NOT a fix-id/count-gate issue and NOT a version
        // limitation. Treat it as a CANARY: it fails loud (promoting to a real
        // assert) the moment TS stops throwing on the overload set, or the prelude
        // stops emitting overloaded rune declares. The Vue carrier (no overloaded
        // declares in its template-binding TSX) IS the real end-to-end remove-all
        // assertion above. Tracked as a follow-up: make the Svelte rune prelude not
        // poison the whole-file fix-all (e.g. non-overloaded rune declares).
        canary_assert_known_limitation!(
            !has_remove_all_unused(&actions, &uri, range),
            "tsserver's getCombinedCodeFix throws `Changes overlap` on the Svelte \
             prelude's overloaded rune `declare function` signatures, so the combined \
             remove-all companion cannot be surfaced for the .svelte carrier today \
             (upstream TypeScript limitation; the Vue carrier proves the path)."
        );
    }
);

// ---------------------------------------------------------------------------
// Vue: add-missing-import quick-fix LANDS in the .vue source (prelude re-anchor)
// ---------------------------------------------------------------------------

/// Does any returned action insert a brand-new `import { <symbol> } from "vue"`
/// statement into the carrier `uri` SOURCE at a SANE position?
///
/// The provider's `addMissingImport` quick-fix inserts the new import at the top
/// of the generated TSX, which lands in Verter's synthetic helper-import preamble;
/// the merge layer must RE-ANCHOR it at the SFC's `<script setup>` import site
/// (never drop it, never line-0 it). The gates:
/// 1. an action whose workspace edit is keyed by the carrier `.vue` URI (never a
///    generated `.tsx`);
/// 2. an INSERTION (zero-width range) whose text imports `symbol` from `"vue"`;
/// 3. the insertion range is NOT `Range::default()` — i.e. it did NOT collapse to
///    line 0 char 0 (the `<script setup>` tag occupies line 0, so the real anchor
///    is the script CONTENT, strictly below it).
fn has_add_missing_vue_import(
    actions: &CodeActionResponse,
    uri: &Uri,
    symbol: &str,
    carrier_source: &str,
    carrier_li: &crate::documents::line_index::LineIndex,
) -> bool {
    assert!(
        !uri.as_str().ends_with(".tsx"),
        "the carrier uri under test must be the .vue source, not a .tsx: {uri:?}"
    );
    // The `<script setup>` content range (typed `scan_sfc_blocks` classification, no string
    // sniffing) — the in-source region the re-anchored import MUST land inside.
    let setup_content = crate::documents::sfc_scanner::scan_sfc_blocks(carrier_source)
        .into_iter()
        .find(|b| b.is_setup())
        .map(|b| b.content_range())
        .expect("the canary fixture has a `<script setup>` block");
    actions.iter().any(|a| {
        let CodeActionOrCommand::CodeAction(ca) = a else {
            return false;
        };
        let Some(edit) = ca.edit.as_ref() else {
            return false;
        };
        let Some(changes) = edit.changes.as_ref() else {
            return false;
        };
        changes.iter().any(|(edit_uri, edits)| {
            edit_uri == uri
                && !edit_uri.as_str().ends_with(".tsx")
                && edits.iter().any(|e| {
                    // A zero-width insertion (start == end) of an `import { symbol } …`
                    // line that references the `vue` module, NOT collapsed to (0,0), AND landing
                    // INSIDE the `<script setup>` content range (the re-anchor target) — never the
                    // file top, never `<template>` or below `</script>`.
                    let in_script_setup = carrier_li
                        .position_to_offset(&e.range.start)
                        .is_some_and(|off| off >= setup_content.0 && off <= setup_content.1);
                    e.range.start == e.range.end
                        && e.range != Range::default()
                        && in_script_setup
                        && e.new_text.contains("import")
                        && e.new_text.contains(symbol)
                        && (e.new_text.contains("from \"vue\"")
                            || e.new_text.contains("from 'vue'"))
                })
        })
    })
}

/// Does the RAW (unmerged) provider output — `getCodeFixes` over the generated TSX, BEFORE
/// `merge_code_actions` — contain an `addMissingImport`-shaped action that inserts `symbol` from the
/// `vue` module? Probes the provider edits directly (keyed to the generated `.tsx`), so the canary
/// can tell whether the PROVIDER emitted the add-import at all — independent of the merge layer.
fn raw_provider_has_add_import(
    actions: &[crate::type_provider::protocol::TypeCodeAction],
    symbol: &str,
) -> bool {
    actions.iter().any(|a| {
        a.edits.iter().any(|e| {
            e.start == e.end
                && e.new_text.contains("import")
                && e.new_text.contains(symbol)
                && (e.new_text.contains("from \"vue\"") || e.new_text.contains("from 'vue'"))
        })
    })
}

real_provider_test!(
    vue_add_missing_import_lands_in_source_via_prelude_reanchor,
    fixture = "single-project",
    async fn run(session) {
        // A `<script setup>` that references `ref` with NO existing `vue` import. Because
        // no `import … from "vue"` statement exists to extend, the provider's
        // `addMissingImport` quick-fix would insert a BRAND-NEW import line at the top of
        // the generated TSX — which lands in Verter's synthetic helper-import preamble.
        // The merge layer re-anchors that prelude insertion at the `<script setup>`
        // import site so the import LANDS in the .vue source (historically dropped).
        let content = "\
<script setup lang=\"ts\">
const base = ref(1)
</script>
<template>
  <div>{{ base }}</div>
</template>
";
        let uri = session
            .open_virtual("src/AddImportQuickFixCase.vue", content)
            .await;
        session.server().test_ensure_synced(&uri).await;

        // The unresolved identifier `ref` (the 2304 site). CTRL+. lands on it.
        let ref_start = carrier_position(session, &uri, "ref(1)", 0);
        let ref_end = carrier_position(session, &uri, "ref(1)", "ref".len());
        let range = Range {
            start: ref_start,
            end: ref_end,
        };

        // Warm the project, then on each attempt probe BOTH layers with the published 2304 in
        // `context.diagnostics` (the wired errorCodes path that drives `getCodeFixes`):
        //   * `provider_emitted` — the RAW provider output (`getCodeFixes` over the generated TSX,
        //     before the merge) contained an add-import for `ref`→`vue`;
        //   * `landed` — the MERGED carrier output placed that import into the `.vue` SOURCE at the
        //     re-anchored `<script setup>` site.
        // Probing both lets the canary distinguish "provider emitted nothing" (harness limitation)
        // from "provider emitted but the merge dropped/mis-placed it" (a real merge regression).
        // Snapshot the carrier source + line index once so the landing check can assert the
        // re-anchored import lands INSIDE the `<script setup>` content range, not merely "not (0,0)".
        let (carrier_source, carrier_li) = {
            let doc = session
                .server()
                .test_documents()
                .get(&uri)
                .expect("document should be open");
            (doc.source.clone(), doc.line_index.clone())
        };

        let mut provider_emitted = false;
        let mut landed = false;
        for attempt in 0..8 {
            let raw = session
                .server()
                .test_raw_provider_code_actions(&uri, range, &[ts2304(range, "ref")])
                .await;
            if raw_provider_has_add_import(&raw, "ref") {
                provider_emitted = true;
            }
            let actions = code_action(session, &uri, range, vec![ts2304(range, "ref")]).await;
            if let Some(actions) = actions {
                if has_add_missing_vue_import(&actions, &uri, "ref", &carrier_source, &carrier_li) {
                    landed = true;
                }
            }
            if provider_emitted || landed {
                break;
            }
            if attempt < 7 {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }
        }

        if provider_emitted {
            // REAL END-TO-END GATE: the provider DID surface an `addMissingImport`. The merge layer
            // MUST re-anchor it into the `.vue` SOURCE — if the import did not land, the merge
            // dropped or mis-placed it (a regression in the prelude re-anchor under test). Fail loud.
            assert!(
                landed,
                "the provider returned an addMissingImport for `ref`→`vue`, but the merge layer did \
                 NOT land it in the .vue source — the prelude re-anchor dropped or mis-placed the \
                 quick-fix edit (regression)."
            );
            return;
        }

        // CANARY (harness limitation): the RAW provider probe confirmed NEITHER backend's
        // `getCodeFixes` surfaces an `addMissingImport` for a carrier (or any in-memory /
        // inferred-project) file here — even though the 2304 is correctly detected. The harness
        // project model + the missing `@verter/types` package mean the provider's import-fix index
        // is unavailable, so there is no add-import to merge. This is now PROVEN per-run by the raw
        // probe above (not assumed): `provider_emitted` stayed false. The merge-layer prelude
        // re-anchor itself is fully exercised by the hermetic unit tests in
        // `type_provider::merge::tests`
        // (`merge_code_actions_add_import_prelude_insertion_reanchors_to_script_setup` plus the
        // Svelte / Vue-no-`<script setup>` / past-preamble / foreign-carrier negative siblings).
        //
        // Because the canary now keys on the RAW provider output, it CANNOT pass vacuously while the
        // merge is broken: the moment a provider returns the add-import here, `provider_emitted`
        // flips true and control takes the real end-to-end assert above (which fails loud if the
        // merge does not land it). At that point delete this canary.
        canary_assert_known_limitation!(
            !landed,
            "neither backend's getCodeFixes surfaced an addMissingImport in this hermetic harness \
             (raw provider probe empty: no import-fix project index / missing @verter/types); the \
             merge-layer prelude re-anchor is proven by the hermetic merge unit tests. When a \
             provider returns the add-import here, the raw probe flips `provider_emitted` and the \
             real end-to-end landing assert fires instead."
        );
    }
);
