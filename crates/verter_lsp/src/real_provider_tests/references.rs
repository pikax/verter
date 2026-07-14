//! References tests ported from E2E suite.

use crate::test_harness::{real_provider_test, RealProviderTestSession};

real_provider_test!(
    references_single_project,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;

        if !session.wait_until_ready(&uri, "action.disabled", 7, "disabled").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        // count → ≥3 references, NOT .tsx
        let pos = session.find_position(&uri, "const count = ref(0)", 6);
        let refs = session.references(&uri, pos).await;
        assert!(refs.len() >= 3, "count should have >= 3 references, got: {}", refs.len());
        for r in &refs {
            let path = RealProviderTestSession::uri_to_path(&r.uri);
            assert!(!path.ends_with(".tsx"), "reference should NOT be in .tsx, got: {path}");
        }

        // increment → ≥2 references
        let pos = session.find_position(&uri, "function increment()", 9);
        let refs = session.references(&uri, pos).await;
        assert!(refs.len() >= 2, "increment should have >= 2 references, got: {}", refs.len());

        // formatCount → ≥2 references, AND the cross-file declaration in utils.ts must carry its
        // REAL range (line 0 of utils.ts: `export function formatCount(...)`), never the line-0
        // placeholder that the merge used to substitute for every cross-file target.
        let pos = session.find_position(&uri, "formatCount", 0);
        let refs = session.references(&uri, pos).await;
        assert!(refs.len() >= 2, "formatCount should have >= 2 references, got: {}", refs.len());

        let utils_ref = refs.iter().find(|r| {
            RealProviderTestSession::uri_to_path(&r.uri).ends_with("utils.ts")
        });
        let utils_ref = utils_ref.unwrap_or_else(|| {
            panic!(
                "formatCount references must include the cross-file declaration in utils.ts; got: {:?}",
                refs.iter().map(|r| RealProviderTestSession::uri_to_path(&r.uri)).collect::<Vec<_>>()
            )
        });
        // `export function formatCount` is on line 0 of utils.ts; the identifier starts at
        // character 16 (`export function ` = 16 chars). Assert the EXACT span — the pre-fix bug
        // collapsed every cross-file target to (0,0), so a zero-length / char-0 range fails here.
        assert_eq!(
            utils_ref.range.start.line, 0,
            "formatCount declaration is on line 0 of utils.ts, got {:?}", utils_ref.range
        );
        assert_eq!(
            utils_ref.range.start.character, 16,
            "formatCount identifier starts at character 16 of utils.ts, got {:?}", utils_ref.range
        );
        assert_eq!(
            utils_ref.range.end.character, 16 + "formatCount".len() as u32,
            "formatCount reference end must span the whole identifier, got {:?}", utils_ref.range
        );
        assert_ne!(
            utils_ref.range, tower_lsp_server::ls_types::Range::default(),
            "cross-file reference must never be the (0,0) line-0 placeholder"
        );
    }
);
