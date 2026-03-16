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

        // formatCount → ≥2 references
        let pos = session.find_position(&uri, "formatCount", 0);
        let refs = session.references(&uri, pos).await;
        assert!(refs.len() >= 2, "formatCount should have >= 2 references, got: {}", refs.len());
    }
);
