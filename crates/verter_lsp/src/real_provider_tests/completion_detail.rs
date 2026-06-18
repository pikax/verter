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
