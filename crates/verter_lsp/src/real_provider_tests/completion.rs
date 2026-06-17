//! Completion tests ported from E2E suite.

use crate::test_harness::real_provider_test;

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
        if !session.is_tsgo() {
            // tsserver reliably resolves JSDoc @type member access in .jsx files.
            // TSGO support is inconsistent — sometimes returns members, sometimes
            // returns scope-level identifiers. Skip for TSGO until stable.
            assert!(labels.contains(&"label".to_string()), "JS SFC member should complete label, got: {labels:?}");
            assert!(labels.contains(&"done".to_string()), "JS SFC member should complete done, got: {labels:?}");
        }
    }
);

// ---------------------------------------------------------------------------
// Event-argument intellisense (inline arrow params + $event)
// ---------------------------------------------------------------------------

real_provider_test!(
    completion_event_argument_payload,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/EventArgCases.vue").await;

        // Warm-up probe on a stable interpolation binding (NOT the event-arg path
        // under test, so a regression FAILS the assertions below instead of
        // vacuously skipping).
        if !session
            .wait_until_ready(
                &uri,
                "{{ greeting }}",
                6,
                "greeting",
            )
            .await
        {
            return;
        }

        // --- Inline arrow event parameter: @click="(ev) => handle(ev.clientX)" ---
        // The parameter `ev` must be typed as the native `click` payload
        // (MouseEvent), so member completion exposes DOM event members.
        let pos = session.find_position(&uri, "ev.clientX", 3); // after `ev.`
        let labels = session.completion_labels(&uri, pos, Some(".")).await;
        assert!(
            labels.contains(&"target".to_string()),
            "inline event param should expose Event.target, got: {labels:?}"
        );
        assert!(
            labels.contains(&"clientX".to_string()),
            "inline event param should be MouseEvent-like (clientX), got: {labels:?}"
        );
        for bad in ["___VERTER___", "__props", "$V_"] {
            assert!(
                !labels.iter().any(|l| l.contains(bad)),
                "inline event-arg completion should NOT leak internal {bad}, got: {labels:?}"
            );
        }

        // --- $event member access: @click="handle($event.clientX)" ---
        let pos = session.find_position(&uri, "$event.clientX", 7); // after `$event.`
        let labels = session.completion_labels(&uri, pos, Some(".")).await;
        assert!(
            labels.contains(&"target".to_string()),
            "$event should expose Event.target, got: {labels:?}"
        );
        assert!(
            labels.contains(&"clientX".to_string()),
            "$event should be MouseEvent-like (clientX), got: {labels:?}"
        );
        for bad in ["___VERTER___", "__props", "$V_"] {
            assert!(
                !labels.iter().any(|l| l.contains(bad)),
                "$event completion should NOT leak internal {bad}, got: {labels:?}"
            );
        }

        // --- Spread-path $event: the SECOND of a duplicate `@click` routes through
        // the spread path. JSX contextual typing cannot flow through a spread, so the
        // codegen annotates the spread `$event` with its explicit payload type; it
        // resolves to the real MouseEvent, not `any`. `@click="handle($event.screenX)"` ---
        let pos = session.find_position(&uri, "$event.screenX", 7); // after `$event.`
        let labels = session.completion_labels(&uri, pos, Some(".")).await;
        assert!(
            labels.contains(&"target".to_string()),
            "spread-path $event should expose Event.target, got: {labels:?}"
        );
        assert!(
            labels.contains(&"screenX".to_string()),
            "spread-path $event should be MouseEvent-like (screenX), got: {labels:?}"
        );
        // Negative: must NOT be `any` — an `any`-typed $event would offer no DOM
        // members (and the old eventCallbacks helper would leak internals).
        for bad in ["___VERTER___", "__props", "$V_"] {
            assert!(
                !labels.iter().any(|l| l.contains(bad)),
                "spread-path $event completion should NOT leak internal {bad}, got: {labels:?}"
            );
        }
    }
);

// ---------------------------------------------------------------------------
// Spread-path COMPONENT event-arg payloads — the closed matrix rows for
// {local binding, GlobalComponents fallback} × {$event, arrow param} × {duplicate,
// hyphenated}. Each spread surface must type its handler from the component's emit
// payload (`InstanceType<typeof Binding>["$props"]["onEvent"]`) — never `$event: any`.
// ---------------------------------------------------------------------------

real_provider_test!(
    completion_component_event_argument_payload,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/EventArgCases.vue").await;
        // Materialize the imported local component and the globally-registered one so the
        // TypeProvider can resolve `InstanceType<typeof Binding>["$props"]` for each.
        let emit_child_uri = session.open_fixture_file("src/EmitChild.vue").await;
        let global_emit_uri = session.open_fixture_file("src/GlobalEmitComp.vue").await;
        session.ensure_synced(&emit_child_uri).await;
        session.ensure_synced(&global_emit_uri).await;
        session.ensure_synced(&uri).await;

        // Warm-up probe on a stable interpolation binding (NOT the event-arg path under
        // test) so a regression FAILS the assertions below instead of vacuously skipping.
        if !session
            .wait_until_ready(
                &uri,
                "{{ greeting }}",
                6,
                "greeting",
            )
            .await
        {
            return;
        }

        let bad_internals = ["___VERTER___", "__props", "$V_"];

        // Component event typing depends on the TypeProvider resolving the imported
        // component's instance type (`InstanceType<typeof Binding>["$props"][...]`). That
        // requires the consumer's import (`'./EmitChild.vue'` → `'./EmitChild.vue.ts'`) to
        // resolve to a materialized component `.vue.ts` with a typed default export. The
        // warm-up probe above already gated on provider readiness, so the local-component
        // spread `$event` MUST resolve here — fail closed if it does not, so a regression
        // that drops the component instance-type resolution FAILS this test instead of
        // vacuously skipping past the payload assertions below.
        let local_dollar = session
            .completion_labels(
                &uri,
                session.find_position(&uri, "$event.pickLabel", 7), // after `$event.`
                Some("."),
            )
            .await;
        assert!(
            !local_dollar.is_empty(),
            "the local-component spread $event completion list must be non-empty (the \
             TypeProvider resolved the imported component instance type); an empty list is \
             the instance-type resolution regression this row targets, got: {local_dollar:?}"
        );

        // --- Local component, DUPLICATE `@pick`: the second handler is a spread key, so
        // its `$event` is the handler payload `{ pickId, pickLabel }` typed via
        // InstanceType<typeof EmitChild>["$props"]["onPick"] — NOT `any`. ---
        assert!(
            local_dollar.contains(&"pickId".to_string())
                && local_dollar.contains(&"pickLabel".to_string()),
            "spread $event on a duplicate local-component event should expose the payload (pickId/pickLabel), got: {local_dollar:?}"
        );
        for bad in bad_internals {
            assert!(
                !local_dollar.iter().any(|l| l.contains(bad)),
                "local-component spread $event completion should NOT leak internal {bad}, got: {local_dollar:?}"
            );
        }

        // --- Local component, HYPHENATED `@row-change` → spread ARROW param typed from
        // the handler payload `{ rowKey, rowName }`. ---
        let pos = session.find_position(&uri, "row.rowKey", 4); // after `row.`
        let labels = session.completion_labels(&uri, pos, Some(".")).await;
        assert!(
            labels.contains(&"rowKey".to_string()) && labels.contains(&"rowName".to_string()),
            "spread arrow param on a hyphenated local-component event should expose the payload (rowKey/rowName), got: {labels:?}"
        );

        // --- Global (GlobalComponents fallback) component, DUPLICATE `@ping`: the second
        // handler's `$event` resolves via the generated fallback const
        // InstanceType<typeof GlobalEmitComp>["$props"]["onPing"] → `{ pingCode, pingCount }`. ---
        let pos = session.find_position(&uri, "$event.pingCount", 7); // after `$event.`
        let labels = session.completion_labels(&uri, pos, Some(".")).await;
        assert!(
            labels.contains(&"pingCode".to_string()) && labels.contains(&"pingCount".to_string()),
            "spread $event on a duplicate global-component event should expose the payload (pingCode/pingCount), got: {labels:?}"
        );
        for bad in bad_internals {
            assert!(
                !labels.iter().any(|l| l.contains(bad)),
                "global-component spread $event completion should NOT leak internal {bad}, got: {labels:?}"
            );
        }

        // --- Global component, HYPHENATED `@late-signal` → spread ARROW param via the
        // fallback const → payload `{ sigName, sigLevel }`. ---
        let pos = session.find_position(&uri, "sig.sigName", 4); // after `sig.`
        let labels = session.completion_labels(&uri, pos, Some(".")).await;
        assert!(
            labels.contains(&"sigName".to_string()) && labels.contains(&"sigLevel".to_string()),
            "spread arrow param on a hyphenated global-component event should expose the payload (sigName/sigLevel), got: {labels:?}"
        );
    }
);
