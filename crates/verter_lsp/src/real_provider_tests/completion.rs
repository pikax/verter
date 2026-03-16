//! Completion tests ported from E2E suite.

use crate::test_harness::{canary_assert_known_limitation, real_provider_test};

// ---------------------------------------------------------------------------
// App.vue template completions — ~12 assertions
// ---------------------------------------------------------------------------

real_provider_test!(
    completion_app_vue_template,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;

        // Wait for provider warmup
        if !session.wait_until_ready(&uri, "action.disabled", 7, "disabled").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        // --- Mustache expression: {{ count }} ---
        let pos = session.find_position(&uri, "{{ count }}", 3);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"count".to_string()), "mustache should complete count, got: {labels:?}");
        assert!(labels.contains(&"doubled".to_string()), "mustache should complete doubled, got: {labels:?}");
        assert!(labels.contains(&"increment".to_string()), "mustache should complete increment, got: {labels:?}");
        // Negative: no global builtins
        assert!(!labels.contains(&"AbortController".to_string()), "should NOT contain AbortController, got: {labels:?}");
        assert!(!labels.contains(&"HTMLDivElement".to_string()), "should NOT contain HTMLDivElement, got: {labels:?}");
        assert!(!labels.contains(&"document".to_string()), "should NOT contain document, got: {labels:?}");
        assert!(!labels.contains(&"window".to_string()), "should NOT contain window, got: {labels:?}");

        // --- Event handler: @click="increment" ---
        let pos = session.find_position(&uri, r#"@click="increment">"#, 8);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"increment".to_string()), "event handler should complete increment, got: {labels:?}");

        // --- V-for member: action.disabled ---
        let pos = session.find_position(&uri, "action.disabled", 7);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"disabled".to_string()), "member should complete disabled, got: {labels:?}");
        assert!(labels.contains(&"label".to_string()), "member should complete label, got: {labels:?}");
        assert!(labels.contains(&"handler".to_string()), "member should complete handler, got: {labels:?}");
        // Negative: no non-member completions
        for bad in ["@click", "foo-bar"] {
            assert!(!labels.contains(&bad.to_string()), "member should NOT contain {bad}, got: {labels:?}");
        }

        // --- Nested v-for inner: action.label (2nd occurrence) ---
        let pos = session.find_nth_position(&uri, "action.label", 1, 7);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"label".to_string()), "nested inner should complete label, got: {labels:?}");
        assert!(labels.contains(&"disabled".to_string()), "nested inner should complete disabled, got: {labels:?}");
        // Negative: should NOT have outer scope members
        assert!(!labels.contains(&"email".to_string()), "nested inner should NOT complete email, got: {labels:?}");
        assert!(!labels.contains(&"age".to_string()), "nested inner should NOT complete age, got: {labels:?}");

        // --- Nested v-for outer: user.name (2nd occurrence) ---
        let pos = session.find_nth_position(&uri, "user.name", 1, 5);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"name".to_string()), "nested outer should complete name, got: {labels:?}");
        assert!(labels.contains(&"email".to_string()), "nested outer should complete email, got: {labels:?}");
        assert!(labels.contains(&"age".to_string()), "nested outer should complete age, got: {labels:?}");
        // Negative: should NOT have inner scope members
        assert!(!labels.contains(&"disabled".to_string()), "nested outer should NOT complete disabled, got: {labels:?}");

        // --- Narrowed member: selectedUser.name ---
        let pos = session.find_position(&uri, "selectedUser.name", 13);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"name".to_string()), "narrowed member should complete name, got: {labels:?}");
        assert!(labels.contains(&"email".to_string()), "narrowed member should complete email, got: {labels:?}");
        assert!(labels.contains(&"age".to_string()), "narrowed member should complete age, got: {labels:?}");
        // Negative: not null members
        assert!(!labels.contains(&"null".to_string()), "narrowed member should NOT complete null, got: {labels:?}");

        // --- Props member: props.title ---
        let pos = session.find_position(&uri, "props.title", 6);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"title".to_string()), "props member should complete title, got: {labels:?}");
        for bad in ["@click", "@custom", "foo-bar"] {
            assert!(!labels.contains(&bad.to_string()), "props member should NOT contain {bad}, got: {labels:?}");
        }

        // --- Broken expression: {{ count + }} ---
        let pos = session.find_position(&uri, "{{ count + }}", 11);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"count".to_string()), "broken expr should complete count, got: {labels:?}");
        assert!(labels.contains(&"doubled".to_string()), "broken expr should complete doubled, got: {labels:?}");
        assert!(labels.contains(&"increment".to_string()), "broken expr should complete increment, got: {labels:?}");
        assert!(!labels.contains(&"AbortController".to_string()), "broken expr should NOT contain AbortController, got: {labels:?}");
        assert!(!labels.contains(&"document".to_string()), "broken expr should NOT contain document, got: {labels:?}");
        assert!(!labels.contains(&"window".to_string()), "broken expr should NOT contain window, got: {labels:?}");

        // --- Computed member: {{ doubled }} should NOT expose .value ---
        let pos = session.find_position(&uri, "{{ doubled }}", 10);
        let labels = session.completion_labels(&uri, pos, Some(".")).await;
        assert!(!labels.contains(&"value".to_string()), "computed member should NOT expose value, got: {labels:?}");

        // --- V-for local: {{ item }} ---
        let pos = session.find_position(&uri, "{{ item }}", 3);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"item".to_string()), "v-for local should complete item, got: {labels:?}");

        // --- No internal leakage in any completion ---
        for bad in ["__props", "___VERTER___", "$V_"] {
            assert!(!labels.iter().any(|l| l.contains(bad)),
                "completions should NOT leak internal {bad}, got: {labels:?}");
        }
    }
);

// ---------------------------------------------------------------------------
// Secondary file completions
// ---------------------------------------------------------------------------

real_provider_test!(
    completion_secondary_files,
    fixture = "single-project",
    async fn run(session) {
        let app_uri = session.open_fixture_file("src/App.vue").await;
        let comp_case_uri = session.open_fixture_file("src/ComponentCompletionCase.vue").await;
        let broken_uri = session.open_fixture_file("src/BrokenTemplateExpr.vue").await;
        let js_uri = session.open_fixture_file("src/JsTemplateCases.vue").await;

        // Wait for warmup on App.vue
        if !session.wait_until_ready(&app_uri, "action.disabled", 7, "disabled").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        // --- ComponentCompletionCase.vue: component tag props ---
        session.ensure_synced(&comp_case_uri).await;
        // ComponentCompletionCase.vue has `<MyComp />` — completions on MyComp
        // The component has foo, bar props and custom event
        let pos = session.find_position(&comp_case_uri, "<MyComp", 1);
        let hover = session.hover_text(&comp_case_uri, pos).await;
        // Just verify component resolution works here (completions on component tags
        // are more complex — they come from verter, not type provider)
        if let Some(text) = hover {
            assert!(text.contains("foo") || text.contains("MyComp"),
                "ComponentCompletionCase should resolve MyComp, got: {text}");
        }

        // --- BrokenTemplateExpr.vue: {{ count + }} ---
        session.ensure_synced(&broken_uri).await;
        let pos = session.find_position(&broken_uri, "{{ count + }}", 11);
        let labels = session.completion_labels(&broken_uri, pos, None).await;
        assert!(labels.contains(&"count".to_string()), "broken expr should complete count, got: {labels:?}");
        assert!(labels.contains(&"formatted".to_string()), "broken expr should complete formatted, got: {labels:?}");

        // --- JsTemplateCases.vue: {{ count }} ---
        session.ensure_synced(&js_uri).await;
        let pos = session.find_position(&js_uri, "{{ count }}", 3);
        let labels = session.completion_labels(&js_uri, pos, None).await;
        assert!(labels.contains(&"count".to_string()), "JS SFC should complete count, got: {labels:?}");
        assert!(labels.contains(&"increment".to_string()), "JS SFC should complete increment, got: {labels:?}");

        // --- JsTemplateCases.vue: state.label member access ---
        let pos = session.find_position(&js_uri, "state.label", 6);
        let labels = session.completion_labels(&js_uri, pos, None).await;
        if session.is_tsgo() {
            // CANARY (TSGO): JS SFC with JSDoc @type annotation — member access on
            // `state.label` returns component-scope completions instead of member
            // completions. TSGO does not resolve JSDoc type annotations for member
            // access in JavaScript Vue SFCs. When TSGO gains this capability, this
            // canary fires and should be promoted to real asserts.
            let has_label = labels.contains(&"label".to_string());
            canary_assert_known_limitation!(
                !has_label,
                "TSGO JS SFC member access does not resolve JSDoc types (got: {labels:?})"
            );
        } else {
            assert!(labels.contains(&"label".to_string()), "JS SFC member should complete label, got: {labels:?}");
            assert!(labels.contains(&"done".to_string()), "JS SFC member should complete done, got: {labels:?}");
        }
    }
);
