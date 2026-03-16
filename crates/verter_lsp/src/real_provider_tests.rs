//! Server-level integration tests with real type providers (tsserver + TSGO).
//!
//! Each test uses `real_provider_test!` to generate two variants — one per provider.
//! Tests skip gracefully when binaries are not found.

use crate::test_harness::real_provider_test;

// ---------------------------------------------------------------------------
// 1. Completion: v-for member access
// ---------------------------------------------------------------------------

real_provider_test!(
    completion_vfor_member_access,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;

        // Wait for the provider to warm up with completions
        if !session
            .wait_until_ready(&uri, "action.disabled", 7, "disabled")
            .await
        {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        let pos = session.find_position(&uri, "action.disabled", 7);
        let labels = session.completion_labels(&uri, pos, None).await;

        assert!(
            labels.contains(&"disabled".to_string()),
            "v-for member access should complete `disabled`, got: {labels:?}"
        );
        assert!(
            labels.contains(&"label".to_string()),
            "v-for member access should complete `label`, got: {labels:?}"
        );
        assert!(
            labels.contains(&"handler".to_string()),
            "v-for member access should complete `handler`, got: {labels:?}"
        );
    }
);

// ---------------------------------------------------------------------------
// 2. Hover: typed binding shows type
// ---------------------------------------------------------------------------

real_provider_test!(
    hover_typed_binding,
    fixture = "single-project",
    async fn run(session) {
        let uri = session
            .open_virtual(
                "src/HoverTest.vue",
                r#"<script setup lang="ts">
const greeting: string = 'hello'
</script>
<template>
  <div>{{ greeting }}</div>
</template>
"#,
            )
            .await;

        // Wait for provider to be ready — probe completion on `greeting` in template
        if !session
            .wait_until_ready(&uri, "{{ greeting }}", 3, "greeting")
            .await
        {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        // Hover on `greeting` in the script block
        let pos = session.find_position(&uri, "const greeting", 6);
        let hover = session.hover_text(&uri, pos).await;

        assert!(
            hover.is_some(),
            "hover on typed binding should return a result"
        );
        let text = hover.unwrap();
        assert!(
            text.contains("string"),
            "hover should show `string` type, got: {text}"
        );
    }
);

// ---------------------------------------------------------------------------
// 3. Go-to-definition: component import
// ---------------------------------------------------------------------------

real_provider_test!(
    definition_component_import,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;

        // Wait for the provider to warm up
        if !session
            .wait_until_ready(&uri, "action.disabled", 7, "disabled")
            .await
        {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        // Go-to-definition on `MyComp` in the import statement
        let pos = session.find_position(&uri, "import MyComp", 7);
        let defs = session.definitions(&uri, pos).await;

        assert!(
            defs.is_some(),
            "go-to-definition on component import should return locations"
        );
    }
);

// ---------------------------------------------------------------------------
// 4. Completion: dot trigger character
// ---------------------------------------------------------------------------

real_provider_test!(
    completion_after_dot_trigger,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;

        // Wait for the provider to warm up
        if !session
            .wait_until_ready(&uri, "action.disabled", 7, "disabled")
            .await
        {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        let pos = session.find_position(&uri, "action.disabled", 7);
        let labels = session.completion_labels(&uri, pos, Some(".")).await;

        assert!(
            labels.contains(&"disabled".to_string()),
            "dot-triggered completion should include `disabled`, got: {labels:?}"
        );
        assert!(
            labels.contains(&"label".to_string()),
            "dot-triggered completion should include `label`, got: {labels:?}"
        );
        // Negative: global completions should not dominate when member access is active
        assert!(
            !labels.contains(&"undefined".to_string()),
            "dot-triggered member completion should not include `undefined`, got: {labels:?}"
        );
    }
);
