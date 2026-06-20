//! Real-provider completion-detail parity tests (tsserver + TSGO).
//!
//! GAP-1: TSGO inherited the `TypeProvider::get_completion_details` trait default
//! (items returned UNCHANGED), so TSGO-backed completion items reached the
//! type-expansion backend (`TypeProviderAdapter::query_members_at_offset`) with no
//! lazy `detail`/`documentation`, while tsserver-backed items carried
//! `completionEntryDetails` enrichment. TSGO now implements
//! `get_completion_details` via per-item `completionItem/resolve`, reaching parity.
//!
//! These exercise the REAL provider's `get_completions` + `get_completion_details`
//! contract directly. Reverting TSGO's `get_completion_details` to the trait
//! default makes the detail assertion fail under TSGO (discriminating).

use crate::test_harness::{real_provider_test, RealProviderTestSession};

/// Pull completions at a `.`-member boundary in an open provider file, retrying
/// while the inferred project warms up.
async fn member_completions_until_nonempty(
    session: &RealProviderTestSession,
    provider_path: &str,
    offset: u32,
) -> Vec<verter_type_runtime::protocol::Completion> {
    for attempt in 0..8 {
        if let Ok(result) = session
            .provider()
            .get_completions(provider_path, offset, Some("."))
            .await
        {
            if !result.items.is_empty() {
                return result.items;
            }
        }
        if attempt < 7 {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// GAP-1: get_completion_details enriches member detail (both providers)
// ---------------------------------------------------------------------------

real_provider_test!(
    completion_detail_enriches_member_signature,
    fixture = "single-project",
    async fn run(session) {
        // A documented, strongly-typed object literal; completing `obj.` yields
        // members whose detail (the `(property) name: type` signature) the bare
        // completion list omits and `get_completion_details` must fill in.
        let source = "\
/** A widget. */
const obj = {
  /** the count */
  count: 123,
  /** the label */
  label: \"hi\",
  /** compute it */
  compute(): number { return this.count; },
};
export const out = obj.;
";
        let path = session
            .open_in_provider("src/__detail_member.tsx", source)
            .await;

        // Position the cursor right after `obj.` in `obj.;`.
        let needle = "obj.;";
        let needle_pos = source.find(needle).expect("needle present in source");
        let offset = (needle_pos + "obj.".len()) as u32;

        let items = member_completions_until_nonempty(session, &path, offset).await;
        if items.is_empty() {
            // Fail-closed: under require-mode (CI sets `VERTER_REQUIRE_TSGO=1`) an
            // empty member list for this controlled `obj.` fixture is a genuine
            // provider/materialization regression and FAILS loudly. Off
            // require-mode it degrades to a recorded skip (local ergonomics).
            if session.allow_empty_result_skip(&format!(
                "provider returned no members for obj. at offset {offset}"
            )) {
                return;
            }
        }

        // The members we expect to see.
        let wants_member = |label: &str| items.iter().any(|i| i.label == label);
        assert!(
            wants_member("count") && wants_member("label") && wants_member("compute"),
            "member completion should include obj's members, got: {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );

        let detailed = session
            .provider()
            .get_completion_details(&path, offset, &items)
            .await
            .expect("get_completion_details should succeed");

        assert_eq!(
            detailed.len(),
            items.len(),
            "detail enrichment must preserve the item list length"
        );

        // The crux of GAP-1: at least one member gains a non-empty `detail`
        // (signature) through enrichment. For TSGO this requires the new
        // `get_completion_details` impl; the trait default returned items
        // unchanged and (for member completions) carried no detail.
        let detail_for = |label: &str| -> Option<String> {
            detailed
                .iter()
                .find(|i| i.label == label)
                .and_then(|i| i.detail.clone())
        };
        let count_detail = detail_for("count");
        let compute_detail = detail_for("compute");
        assert!(
            count_detail.as_deref().is_some_and(|d| !d.is_empty())
                || compute_detail.as_deref().is_some_and(|d| !d.is_empty()),
            "get_completion_details must enrich at least one member with a non-empty \
             detail signature (GAP-1); count={count_detail:?} compute={compute_detail:?}"
        );

        // When `count`'s detail is present it must describe a numeric property —
        // proving the enrichment carried the real resolved signature, not a stub.
        if let Some(d) = count_detail {
            assert!(
                d.contains("count") && d.contains("number"),
                "enriched detail for `count` should describe its type, got: {d:?}"
            );
        }

        // Documentation parity: the bare member list omits the JSDoc, and the
        // resolve enrichment must recover it (matching tsserver). At least one
        // member's resolved documentation carries its JSDoc text.
        let doc_for = |label: &str| -> Option<String> {
            detailed
                .iter()
                .find(|i| i.label == label)
                .and_then(|i| i.documentation.clone())
        };
        assert!(
            doc_for("count")
                .as_deref()
                .is_some_and(|d| d.contains("the count"))
                || doc_for("label")
                    .as_deref()
                    .is_some_and(|d| d.contains("the label"))
                || doc_for("compute")
                    .as_deref()
                    .is_some_and(|d| d.contains("compute it")),
            "get_completion_details must recover member JSDoc documentation (GAP-1); \
             count={:?} label={:?} compute={:?}",
            doc_for("count"),
            doc_for("label"),
            doc_for("compute"),
        );
    }
);

// ---------------------------------------------------------------------------
// completionItem.resolveSupport[additionalTextEdits] — auto-import on resolve
// ---------------------------------------------------------------------------

// Auto-import edits ride `completionItem/resolve.additionalTextEdits`, which an
// LSP server (tgo) computes lazily ONLY when the client advertises
// `textDocument.completion.completionItem.resolveSupport.properties` containing
// `additionalTextEdits`. Before that capability was advertised, tgo's near-empty
// handshake made it silently drop the import edit, so accepting an auto-import
// completion inserted the identifier WITHOUT its `import { … }` statement. (The
// resolve `data` blob rides every round-trip transparently per the LSP spec — no
// `dataSupport` capability exists or is needed.)
//
// This drives the REAL provider end to end: a workspace sibling exports a unique
// symbol; a second file references it WITHOUT importing; the provider's
// completion for that symbol must, on resolve, carry an `additionalTextEdits`
// edit whose text is the missing import. The macro runs this for BOTH providers
// (tsserver already advertised resolve via its native protocol; tgo needs the
// new capability) and vacuously skips when the backend / node_modules is absent.
//
// Discriminating for tgo: reverting the `resolveSupport` capability makes tgo
// return no `additionalTextEdits` on resolve, so the import-edit assertion fails
// (under require-mode) instead of passing.
real_provider_test!(
    completion_resolve_carries_auto_import_edit,
    fixture = "single-project",
    async fn run(session) {
        // A workspace sibling that exports a uniquely-named symbol. Auto-import
        // must offer it (and, on resolve, supply the import statement) in a file
        // that references the name without importing it.
        let marker = "verterAutoImportMarker42";
        let src = format!("export const {marker} = 123;\n");
        let _src_path = session
            .open_in_provider("src/__verter_autoimport_src.tsx", &src)
            .await;

        // The use site references the marker with no import — the completion list
        // for the partial identifier should include the cross-file auto-import.
        let use_src = format!("export const usage = {marker};\n");
        let use_path = session
            .open_in_provider("src/__verter_autoimport_use.tsx", &use_src)
            .await;

        // Cursor at the END of the typed identifier so the completion list is the
        // identifier completion (the auto-import candidate carries the import edit).
        let needle_pos = use_src.find(marker).expect("marker present in use source");
        let offset = (needle_pos + marker.len()) as u32;

        // Retry while the project indexes the sibling export (auto-import needs the
        // workspace symbol table warm).
        let mut import_edit_text: Option<String> = None;
        'outer: for attempt in 0..10 {
            let Ok(result) = session
                .provider()
                .get_completions(&use_path, offset, None)
                .await
            else {
                if attempt < 9 {
                    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                }
                continue;
            };

            // Find the auto-import candidate for our marker that carries a resolve
            // handle, then resolve it and inspect its additionalTextEdits.
            for item in result.items.iter().filter(|i| i.label == marker) {
                let Some(data) = item.data.clone() else {
                    continue;
                };
                if let Ok(Some(resolved)) =
                    session.provider().resolve_completion(&use_path, data).await
                {
                    // Capture only the edit that IS the import of our marker — both
                    // `import` and the marker name. A looser `||` could latch onto a
                    // sibling import edit (text with `import` but not the marker) and
                    // break early, then fail the stricter `&&` assertion below.
                    for edit in &resolved.additional_text_edits {
                        if edit.new_text.contains("import") && edit.new_text.contains(marker) {
                            import_edit_text = Some(edit.new_text.clone());
                            break 'outer;
                        }
                    }
                }
            }
            if attempt < 9 {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }
        }

        match import_edit_text {
            Some(text) => {
                // The resolved edit must be the import STATEMENT for the sibling
                // symbol — proving tgo computed `additionalTextEdits` because the
                // client advertised the resolveSupport capability. Strengthen past
                // the capture predicate (which only required `import` + the marker
                // substrings): a real auto-import for a cross-file symbol is
                // `import { <marker> } from "<module>"` (or `import <marker> from
                // …`), so it MUST also carry the `from` module-specifier clause.
                // A bare text that merely contains both substrings (e.g. a comment
                // or a side-effect `import "<marker>";`) lacks `from` and fails —
                // discrimination the capture condition does not already guarantee.
                assert!(
                    text.contains("from") && text.contains(marker),
                    "auto-import resolve edit should be the import statement \
                     `import {{ {marker} }} from \"…\"` (with a `from` module clause), \
                     got: {text:?}"
                );
            }
            None => {
                // Fail-closed under require-mode (CI: `allow_empty_result_skip`
                // panics), recorded-skip otherwise — an absent import edit for this
                // controlled workspace symbol is a genuine resolveSupport regression
                // when the backend is present.
                let _skipped = session.allow_empty_result_skip(&format!(
                    "no completionItem/resolve additionalTextEdits import edit surfaced for \
                     workspace symbol {marker} (provider={}, auto-import indexing may be cold)",
                    if session.is_tsgo() { "tsgo" } else { "tsserver" }
                ));
            }
        }
    }
);
