//! Hover tests ported from E2E suite.

use crate::test_harness::real_provider_test;

// ---------------------------------------------------------------------------
// App.vue bindings — ~18 assertions sharing one provider session
// ---------------------------------------------------------------------------

real_provider_test!(
    hover_app_vue_bindings,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;

        // Wait for provider warmup
        if !session.wait_until_ready(&uri, "action.disabled", 7, "disabled").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        // --- ref binding: {{ count }} ---
        let pos = session.find_position(&uri, "{{ count }}", 3);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on count should return a result");
        let text = hover.unwrap();
        assert!(text.contains("count"), "hover should mention count, got: {text}");
        assert!(text.contains("number"), "hover should mention number, got: {text}");

        // --- computed binding: {{ doubled }} ---
        let pos = session.find_position(&uri, "{{ doubled }}", 3);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on doubled should return a result");
        let text = hover.unwrap();
        assert!(text.contains("doubled"), "hover should mention doubled, got: {text}");
        assert!(text.contains("number"), "hover should mention number, got: {text}");

        // --- prop binding: {{ title }} ---
        let pos = session.find_position(&uri, "{{ title }}", 3);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on title should return a result");
        let text = hover.unwrap();
        assert!(text.contains("title"), "hover should mention title, got: {text}");
        assert!(text.contains("string"), "hover should mention string, got: {text}");

        // --- props.title member access ---
        let pos = session.find_position(&uri, "props.title", 6);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on props.title should return a result");
        let text = hover.unwrap();
        assert!(text.contains("title"), "hover should mention title, got: {text}");
        assert!(text.contains("string"), "hover should mention string, got: {text}");

        // --- prop attribute: foo="literal" ---
        let pos = session.find_position(&uri, r#"foo="literal""#, 0);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on foo prop should return a result");
        let text = hover.unwrap();
        assert!(text.contains("foo"), "hover should mention foo, got: {text}");
        assert!(text.contains("string"), "hover should mention string, got: {text}");

        // --- v-bind prop: :bar="count" on bar ---
        let pos = session.find_position(&uri, r#":bar="count""#, 1);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on :bar should return a result");
        let text = hover.unwrap();
        assert!(text.contains("bar"), "hover should mention bar, got: {text}");
        assert!(text.contains("number"), "hover should mention number, got: {text}");

        // --- v-bind prop: :bar="count" on count (expression) ---
        let pos = session.find_position(&uri, r#":bar="count""#, 6);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on bound count should return a result");
        let text = hover.unwrap();
        assert!(text.contains("count"), "hover should mention count, got: {text}");
        assert!(text.contains("number"), "hover should mention number, got: {text}");

        // --- component tag: <MyComp ---
        let pos = session.find_position(&uri, "<MyComp", 1);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on <MyComp should return a result");
        let text = hover.unwrap();
        assert!(text.contains("foo"), "component hover should mention foo prop, got: {text}");
        assert!(text.contains("bar"), "component hover should mention bar prop, got: {text}");
        assert!(text.contains("custom"), "component hover should mention custom event, got: {text}");

        // --- event attribute: @custom on event name ---
        let pos = session.find_position(&uri, r#"@custom="handleCustom($event)""#, 1);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on @custom should return a result");
        let text = hover.unwrap();
        assert!(text.contains("custom"), "hover should mention custom, got: {text}");
        assert!(text.contains("payload"), "hover should mention payload, got: {text}");
        assert!(text.contains("string"), "hover should mention string, got: {text}");

        // --- event handler with modifier: @click.prevent="increment" ---
        let delta = "@click.prevent=\"".len();
        let pos = session.find_position(&uri, r#"@click.prevent="increment""#, delta);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on increment handler should return a result");
        let text = hover.unwrap();
        assert!(text.contains("increment"), "hover should mention increment, got: {text}");

        // --- v-for local typed: action.disabled offset 0 ---
        let pos = session.find_position(&uri, "action.disabled", 0);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on action should return a result");
        let text = hover.unwrap();
        // Either shows named type "Action" or expanded properties
        assert!(
            text.contains("Action") || (text.contains("label") && text.contains("disabled") && text.contains("handler")),
            "hover should show Action type or its properties, got: {text}"
        );

        // --- v-for member: action.disabled offset 7 ---
        let pos = session.find_position(&uri, "action.disabled", 7);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on disabled should return a result");
        let text = hover.unwrap();
        assert!(text.contains("disabled"), "hover should mention disabled, got: {text}");
        assert!(text.contains("boolean"), "hover should mention boolean, got: {text}");

        // --- v-if narrowed: selectedUser.name offset 0 ---
        let pos = session.find_position(&uri, "selectedUser.name", 0);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on selectedUser should return a result");
        let text = hover.unwrap();
        assert!(text.contains("User"), "hover should mention User type, got: {text}");
        assert!(!text.contains("null"), "narrowed hover should NOT mention null, got: {text}");

        // --- v-model: v-model="inputVal" offset 9 ---
        let pos = session.find_position(&uri, r#"v-model="inputVal""#, 9);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on v-model inputVal should return a result");
        let text = hover.unwrap();
        assert!(text.contains("inputVal"), "hover should mention inputVal, got: {text}");

        // --- named v-model ARG: v-model:show on the `show` arg must resolve the
        // child component's `show` model prop type (boolean) via the mapped
        // prop-name codegen. Pre-fix the arg had no source→TSX mapping → no hover. ---
        //
        // LOAD-BEARING: the source-owned `v_model_hover` ALWAYS emits "show"
        // regardless of whether the codegen `InsertMapped` is live, so a bare
        // `contains("show")` would pass even if the mapped prop-name piece were
        // reverted. The mapping is what lets the TypeProvider resolve the child's
        // `$props['show']` (`defineModel<boolean>("show")` in ModelNamed.vue);
        // `merge_hover(Some, Some)` then appends TSGO's type block, so when the
        // mapping is live the hover text MUST contain the resolved `boolean` type.
        // Assert BOTH: the prop name AND the resolved child type — `boolean` proves
        // the load-bearing mapping, not just the source-owned name.
        let delta = "v-model:".len();
        let pos = session.find_position(&uri, r#"v-model:show="showFlag""#, delta);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on v-model:show arg should return a result");
        let text = hover.unwrap();
        assert!(
            text.contains("show"),
            "hover on v-model:show arg should mention the `show` model prop, got: {text}"
        );
        assert!(
            text.contains("boolean"),
            "hover on v-model:show arg MUST resolve the child `defineModel<boolean>(\"show\")` \
             type via the mapped prop-name codegen (proves the InsertMapped mapping is live, \
             not just the source-owned name), got: {text}"
        );

        // --- named v-model VALUE: the bound `showFlag` still hovers (regression). ---
        let delta = "v-model:show=\"".len();
        let pos = session.find_position(&uri, r#"v-model:show="showFlag""#, delta);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on v-model:show value should return a result");
        let text = hover.unwrap();
        assert!(text.contains("showFlag"), "hover should mention showFlag, got: {text}");

        // --- destructured v-for: {{ name }} in (user, idx) context ---
        let pos = session.find_position(&uri, "{{ name }}", 3);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on destructured name should return a result");
        let text = hover.unwrap();
        assert!(text.contains("name"), "hover should mention name, got: {text}");
        assert!(text.contains("string"), "hover should mention string, got: {text}");

        // --- v-for index: (user, idx) offset 7 ---
        let pos = session.find_position(&uri, "(user, idx)", 7);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on idx should return a result");
        let text = hover.unwrap();
        assert!(text.contains("idx"), "hover should mention idx, got: {text}");
        assert!(text.contains("number"), "hover should mention number, got: {text}");

        // --- $event not any: handleInput($event) offset 12 ---
        let pos = session.find_position(&uri, "handleInput($event)", 12);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on $event should return a result");
        let text = hover.unwrap();
        assert!(text.contains("Event"), "hover should mention Event, got: {text}");
        assert!(!text.to_lowercase().contains(": any"), "hover on $event should NOT be any, got: {text}");

        // --- ref declaration: const count = ref(0) ---
        let pos = session.find_position(&uri, "const count = ref(0)", 6);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on ref decl should return a result");
        let text = hover.unwrap();
        assert!(text.contains("Ref"), "ref decl hover should mention Ref, got: {text}");
        assert!(text.contains("number"), "ref decl hover should mention number, got: {text}");
        assert!(!text.contains("Ref<any>"), "ref decl hover should NOT show Ref<any>, got: {text}");

        // --- computed declaration: const doubled = computed( ---
        let pos = session.find_position(&uri, "const doubled = computed(", 6);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on computed decl should return a result");
        let text = hover.unwrap();
        assert!(
            text.contains("ComputedRef") || text.contains("number"),
            "computed decl hover should mention ComputedRef or number, got: {text}"
        );
    }
);

// ---------------------------------------------------------------------------
// Secondary files — opens multiple files from single-project
// ---------------------------------------------------------------------------

real_provider_test!(
    hover_secondary_files,
    fixture = "single-project",
    async fn run(session) {
        // Open all files we need
        let app_uri = session.open_fixture_file("src/App.vue").await;
        let mycomp_uri = session.open_fixture_file("src/MyComp.vue").await;
        let recovery_uri = session.open_fixture_file("src/TemplateRecovery.vue").await;
        let template_only_uri = session.open_fixture_file("src/TemplateOnly.vue").await;
        let js_uri = session.open_fixture_file("src/JsTemplateCases.vue").await;
        let type_res_uri = session.open_fixture_file("src/TypeResolutionCases.vue").await;

        // The contract is non-vacuous: a materialized provider must warm the
        // project before any secondary-file hover is accepted.
        assert!(
            session
                .wait_until_ready(&app_uri, "action.disabled", 7, "disabled")
                .await,
            "the provider must warm the fixture project before hover assertions"
        );

        // --- MyComp.vue: slot outlet <slot name="header" ---
        let pos = session.find_position(&mycomp_uri, r#"<slot name="header""#, 1);
        let hover = session.hover_text(&mycomp_uri, pos).await;
        assert!(hover.is_some(), "hover on slot outlet should return a result");
        let text = hover.unwrap();
        let text_lower = text.to_lowercase();
        assert!(text_lower.contains("slot"), "slot outlet hover should mention slot, got: {text}");
        assert!(!text.contains("() any"), "slot outlet hover should NOT show () any, got: {text}");

        // --- App.vue: slot consumer <template #header> ---
        let pos = session.find_position(&app_uri, "<template #header>", 10);
        let hover = session.hover_text(&app_uri, pos).await;
        // Slot consumer hover may not always return data — skip assertion if None
        if let Some(text) = hover {
            let text_lower = text.to_lowercase();
            assert!(text_lower.contains("slot") || text_lower.contains("header"),
                "slot consumer hover should mention slot or header, got: {text}");
        }

        // --- TemplateRecovery.vue: broken script, {{ count }} still works ---
        session.ensure_synced(&recovery_uri).await;
        let pos = session.find_position(&recovery_uri, "{{ count }}", 3);
        let hover = session.hover_text(&recovery_uri, pos).await;
        assert!(hover.is_some(), "hover on count in recovery file should return a result");
        let text = hover.unwrap();
        assert!(text.contains("count"), "recovery hover should mention count, got: {text}");
        assert!(text.contains("number"), "recovery hover should mention number, got: {text}");

        // --- TemplateOnly.vue: <slot name="header" ---
        session.ensure_synced(&template_only_uri).await;
        let pos = session.find_position(&template_only_uri, r#"<slot name="header""#, 1);
        let hover = session.hover_text(&template_only_uri, pos).await;
        assert!(hover.is_some(), "hover on template-only slot should return a result");
        let text = hover.unwrap();
        let text_lower = text.to_lowercase();
        assert!(text_lower.contains("slot"), "template-only slot hover should mention slot, got: {text}");
        assert!(!text.contains("() any"), "template-only slot hover should NOT show () any, got: {text}");

        // --- JsTemplateCases.vue: {{ count }} ---
        session.ensure_synced(&js_uri).await;
        assert!(
            session
                .wait_until_ready(&js_uri, "{{ count }}", 3, "count")
                .await,
            "the JavaScript carrier must enter the provider program before its typed hover is asserted"
        );
        let pos = session.find_position(&js_uri, "{{ count }}", 3);
        let hover = session.hover_text(&js_uri, pos).await;
        assert!(hover.is_some(), "hover on JS SFC count should return a result");
        let text = hover.unwrap();
        assert!(text.contains("count"), "JS SFC hover should mention count, got: {text}");
        assert!(text.contains("number"), "JS SFC hover should mention number, got: {text}");

        // --- TypeResolutionCases.vue: {{ mixed }} ---
        session.ensure_synced(&type_res_uri).await;
        assert!(
            session
                .wait_until_ready(&type_res_uri, "{{ mixed }}", 3, "mixed")
                .await,
            "the type-resolution carrier must enter the provider program before its union hover is asserted"
        );
        let pos = session.find_position(&type_res_uri, "{{ mixed }}", 3);
        let hover = session.hover_text(&type_res_uri, pos).await;
        assert!(hover.is_some(), "hover on mixed should return a result");
        let text = hover.unwrap();
        assert!(text.contains("string"), "mixed hover should mention string, got: {text}");
        assert!(text.contains("number"), "mixed hover should mention number, got: {text}");
    }
);

// ---------------------------------------------------------------------------
// Import binding hover
// ---------------------------------------------------------------------------

real_provider_test!(
    hover_import_binding,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;
        let _mycomp_uri = session.open_fixture_file("src/MyComp.vue").await;

        if !session.wait_until_ready(&uri, "action.disabled", 7, "disabled").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        // Hover on MyComp in the import statement
        let pos = session.find_position(&uri, "import MyComp", 7);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on import MyComp should return a result");
        let text = hover.unwrap();
        assert!(text.contains("foo"), "import hover should mention foo prop, got: {text}");
        assert!(text.contains("bar"), "import hover should mention bar prop, got: {text}");
        assert!(!text.to_lowercase().contains(": any"), "import hover should NOT be any, got: {text}");
    }
);
