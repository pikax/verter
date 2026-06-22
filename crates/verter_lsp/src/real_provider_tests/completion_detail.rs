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
// SCOPE: this proves PLAIN TypeScript configured-project auto-import resolve
// parity — a real on-disk `.ts` use-site importing a real on-disk `.ts` sibling.
// It does NOT cover the framework CARRIER surface (`.vue`/`.svelte` → generated
// virtual TSX), whose project membership for tsserver is a SEPARATE concern and
// is exercised by the carrier E2E path, not this direct-provider test.
//
// It drives the REAL provider end to end for that plain-TS shape: a workspace
// sibling (`src/utils.ts`) exports `formatCount`, and an on-disk use-site
// (`src/AutoImportFormatCount.ts`) references it WITHOUT importing. Because BOTH
// files physically exist under the fixture's tsconfig `include: ["src"]`, they
// are CONFIGURED-PROJECT members on both providers — the shape under which a
// plain-TS use-site's auto-import map sees its configured-project siblings. (An
// in-memory-only sibling at a synthetic path lands in an *inferred* project whose
// auto-import map excludes configured-project siblings, so tsserver returns no
// import edit there — that is the wrong shape for a plain-TS use-site, whose real
// import source is a real on-disk tsconfig member.) The provider's completion for
// `formatCount` must, on resolve, carry an `additionalTextEdits` edit whose text
// is the missing `import { formatCount } from "./utils"`.
//
// The macro runs this for BOTH providers and vacuously skips when the backend /
// node_modules is absent.
//
// Discriminating: reverting the `resolveSupport` capability (tgo) or neutering
// the provider's `resolve_completion` additionalTextEdits mapping (tsserver)
// makes resolve return no import edit, so the assertion fails (under require-mode)
// instead of passing.
real_provider_test!(
    completion_resolve_carries_auto_import_edit,
    fixture = "single-project",
    async fn run(session) {
        // The auto-import SOURCE: the committed on-disk workspace export
        // `formatCount` from `src/utils.ts` (a configured-project member also
        // resolved cross-file by the definition/references real-provider tests).
        let symbol = "formatCount";
        // Open the export source for its side effect only (configured-project
        // membership) — the test does not need its path or content.
        let (_, _) = session.open_fixture_in_provider("src/utils.ts").await;

        // The USE-SITE: a committed on-disk `.ts` that references `formatCount`
        // without importing it. Opening it from disk keeps it a configured-project
        // member (the realistic shape) — not an inferred-project in-memory buffer.
        // The harness owns the disk read and hands back the content, so the test
        // body needs no `std::fs` of its own (the VFS-boundary guard keeps direct
        // OS file APIs inside the test-fixture-read boundary).
        let (use_path, use_src) = session
            .open_fixture_in_provider("src/AutoImportFormatCount.ts")
            .await;

        // Cursor at the END of the `formatCount` identifier in the CODE (the
        // `formatCount(...)` call), not its mentions in the file's leading comment.
        // The completion list there is the identifier completion (the auto-import
        // candidate carries the import edit).
        let call_needle = format!("{symbol}(");
        let needle_pos = use_src
            .find(&call_needle)
            .expect("formatCount call present in use source");
        let offset = (needle_pos + symbol.len()) as u32;

        // Retry while the project indexes the sibling export (auto-import needs the
        // workspace symbol table warm).
        let mut import_edit_text: Option<String> = None;
        'outer: for attempt in 0..8 {
            let Ok(result) = session
                .provider()
                .get_completions(&use_path, offset, None)
                .await
            else {
                if attempt < 7 {
                    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                }
                continue;
            };

            // Find the auto-import candidate for our symbol that carries a resolve
            // handle, then resolve it and inspect its additionalTextEdits.
            for item in result.items.iter().filter(|i| i.label == symbol) {
                let Some(data) = item.data.clone() else {
                    continue;
                };
                if let Ok(Some(resolved)) =
                    session.provider().resolve_completion(&use_path, data).await
                {
                    // Capture only the edit that IS the import of our symbol — both
                    // `import` and the symbol name. A looser `||` could latch onto a
                    // sibling import edit (text with `import` but not the symbol) and
                    // break early, then fail the stricter `&&` assertion below.
                    for edit in &resolved.additional_text_edits {
                        if edit.new_text.contains("import") && edit.new_text.contains(symbol) {
                            import_edit_text = Some(edit.new_text.clone());
                            break 'outer;
                        }
                    }
                }
            }
            if attempt < 7 {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }
        }

        match import_edit_text {
            Some(text) => {
                // The resolved edit must be the import STATEMENT for the sibling
                // symbol — proving the provider computed `additionalTextEdits`.
                // Strengthen past the capture predicate (which only required
                // `import` + the symbol substrings): a real auto-import for a
                // cross-file symbol is `import { formatCount } from "<module>"`, so
                // it MUST also carry the `from` module-specifier clause AND name the
                // source module (`./utils`). A bare text that merely contains both
                // substrings (e.g. a comment or a side-effect `import "…";`) lacks
                // `from` and fails — discrimination the capture condition does not
                // already guarantee.
                assert!(
                    text.contains("from") && text.contains(symbol),
                    "auto-import resolve edit should be the import statement \
                     `import {{ {symbol} }} from \"…\"` (with a `from` module clause), \
                     got: {text:?}"
                );
                // Pin the exact module-specifier clause `from "./utils"` (allowing
                // either quote style), NOT a bare `utils` substring: an unrelated
                // module whose path merely contains `utils` must NOT satisfy this.
                assert!(
                    text.contains("from \"./utils\"") || text.contains("from './utils'"),
                    "auto-import resolve edit should reference the source module \
                     `./utils` (where {symbol} is defined) via a `from \"./utils\"` \
                     clause, got: {text:?}"
                );
            }
            None => {
                // Fail-closed under require-mode (CI: `allow_empty_result_skip`
                // panics), recorded-skip otherwise — an absent import edit for this
                // controlled workspace symbol is a genuine resolveSupport regression
                // when the backend is present.
                let _skipped = session.allow_empty_result_skip(&format!(
                    "no completionItem/resolve additionalTextEdits import edit surfaced for \
                     configured-project workspace symbol {symbol} (provider={})",
                    if session.is_tsgo() { "tsgo" } else { "tsserver" }
                ));
            }
        }
    }
);

// Negative guard: the resolve path must NOT fabricate an import edit for a symbol
// that needs none. A LOCAL declaration referenced in the same file is fully
// resolved without any import, so resolving its completion must carry NO
// `import … from …` additionalTextEdits. This proves the auto-import RESOLVE
// plumbing is driven by the provider's real codeActions, not by a heuristic that
// invents an import for any completed identifier (which would FAIL this test).
real_provider_test!(
    completion_resolve_does_not_fabricate_import_for_local_symbol,
    fixture = "single-project",
    async fn run(session) {
        // A self-contained file: `localOnlySymbol` is declared and referenced in
        // the SAME module, so completing it needs no import.
        let local = "localOnlySymbol";
        let src = format!(
            "const {local} = 123;\nexport const localUsage = {local};\n"
        );
        let path = session
            .open_in_provider("src/__verter_local_only.tsx", &src)
            .await;

        // Cursor at the end of the second (usage) occurrence of the local name.
        let first = src.find(local).expect("declaration present");
        let usage_pos = src[first + local.len()..]
            .find(local)
            .map(|rel| first + local.len() + rel)
            .expect("usage occurrence present");
        let offset = (usage_pos + local.len()) as u32;

        // Resolve every completion candidate for the local name and assert none of
        // them carries a fabricated import edit. The guard FIRES on the resolved
        // edit set: if the resolve path wrongly invented `import … from …` for a
        // symbol that needs none, this assertion fails. (A provider correctly
        // attaches NO resolve handle to a purely-local symbol — tgo does this, so
        // its resolve never runs and trivially fabricates nothing — while tsserver
        // does resolve the local entry, exercising the assertion live.)
        let mut saw_local_candidate = false;
        let mut checked_resolved_edits = false;
        for attempt in 0..6 {
            let Ok(result) = session.provider().get_completions(&path, offset, None).await else {
                if attempt < 5 {
                    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                }
                continue;
            };

            for item in result.items.iter().filter(|i| i.label == local) {
                saw_local_candidate = true;
                let Some(data) = item.data.clone() else {
                    continue;
                };
                if let Ok(Some(resolved)) = session.provider().resolve_completion(&path, data).await {
                    checked_resolved_edits = true;
                    for edit in &resolved.additional_text_edits {
                        // Reject ANY import-looking edit, not just a `import … from …`
                        // pair: a fabricated side-effect `import "…";` (no `from`)
                        // would also be a wrongful import for a symbol that needs none.
                        assert!(
                            !edit.new_text.contains("import"),
                            "resolve must NOT fabricate an import for the local symbol \
                             {local} (it needs none); got edit: {:?}",
                            edit.new_text
                        );
                    }
                }
            }
            if saw_local_candidate {
                break;
            }
            if attempt < 5 {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }
        }

        // Under require-mode the completion path MUST have surfaced the local
        // symbol (proving the negative guard ran against a real result set), else
        // it is a provider/materialization regression. Whether the resolve leg
        // fired depends on the provider attaching a handle to a no-import symbol —
        // both outcomes (resolved-with-no-import on tsserver, no-handle on tgo) are
        // correct and the import-fabrication assertion covers the resolved case.
        if !saw_local_candidate {
            let _skipped = session.allow_empty_result_skip(&format!(
                "local symbol {local} never surfaced as a completion candidate \
                 (provider={})",
                if session.is_tsgo() { "tsgo" } else { "tsserver" }
            ));
        }

        // tsserver attaches a resolve handle to the local entry too, so its resolve
        // leg MUST have fired — proving the no-fabrication assertion ran against a
        // REAL resolved-edit set rather than vacuously (a `data: None` regression
        // would skip the resolve and silently neuter this guard). tgo correctly
        // attaches no handle to a purely-local symbol, so it has nothing to resolve.
        if saw_local_candidate && !session.is_tsgo() {
            assert!(
                checked_resolved_edits,
                "tsserver must resolve the local candidate {local} so the \
                 no-fabricated-import guard is exercised against real resolved edits"
            );
        }
    }
);
