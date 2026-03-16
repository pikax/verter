//! Document symbols tests ported from E2E suite.

use crate::test_harness::real_provider_test;

real_provider_test!(
    document_symbols_single_project,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;

        if !session.wait_until_ready(&uri, "action.disabled", 7, "disabled").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        let names = session.document_symbols(&uri).await;
        assert!(!names.is_empty(), "should have at least one symbol");
        assert!(names.contains(&"count".to_string()), "should contain count, got: {names:?}");
        assert!(names.contains(&"doubled".to_string()), "should contain doubled, got: {names:?}");
        assert!(names.contains(&"increment".to_string()), "should contain increment, got: {names:?}");
    }
);
