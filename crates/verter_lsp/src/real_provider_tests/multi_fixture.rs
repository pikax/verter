//! Multi-fixture tests — each spawns its own provider for a different fixture.

use crate::test_harness::real_provider_test;

// ---------------------------------------------------------------------------
// Monorepo: cross-package component
// ---------------------------------------------------------------------------

real_provider_test!(
    hover_monorepo,
    fixture = "monorepo",
    async fn run(session) {
        let uri = session.open_fixture_file("packages/app/src/App.vue").await;
        let _shared = session.open_fixture_file("packages/shared/src/SharedComp.vue").await;

        if !session.wait_until_ready(&uri, "{{ count }}", 3, "count").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        let pos = session.find_position(&uri, "<SharedComp", 1);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on SharedComp should return a result");
        let text = hover.unwrap();
        assert!(text.contains("foo"), "SharedComp hover should mention foo, got: {text}");
        assert!(text.contains("bar"), "SharedComp hover should mention bar, got: {text}");
    }
);

// ---------------------------------------------------------------------------
// Composite paths: @/ alias with composite project
// ---------------------------------------------------------------------------

real_provider_test!(
    hover_composite_paths,
    fixture = "composite-paths",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;
        let _hw = session.open_fixture_file("src/components/HelloWorld.vue").await;

        if !session.wait_until_ready(&uri, "{{ count }}", 3, "count").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        let pos = session.find_position(&uri, "<HelloWorld", 1);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on HelloWorld should return a result");
        let text = hover.unwrap();
        assert!(text.contains("msg"), "HelloWorld hover should mention msg, got: {text}");
    }
);

// ---------------------------------------------------------------------------
// Path aliases: @/ alias
// ---------------------------------------------------------------------------

real_provider_test!(
    hover_path_aliases,
    fixture = "path-aliases",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;
        let _mycomp = session.open_fixture_file("src/components/MyComp.vue").await;

        if !session.wait_until_ready(&uri, "{{ count }}", 3, "count").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        let pos = session.find_position(&uri, "<MyComp", 1);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on MyComp should return a result");
        let text = hover.unwrap();
        assert!(text.contains("foo"), "MyComp hover should mention foo, got: {text}");
    }
);

// ---------------------------------------------------------------------------
// Path aliases through a barrel re-export: @/ alias + `export { default as X }`
// ---------------------------------------------------------------------------

real_provider_test!(
    hover_aliased_barrel,
    fixture = "path-aliases",
    async fn run(session) {
        // AppBarrel imports MyComp via `import { MyComp } from '@/components'`, where
        // `src/components/index.ts` re-exports `./MyComp.vue`. Exercises the eager barrel BFS over
        // an alias-resolved barrel hop end-to-end (real tsconfig `paths`): the `@/` specifier must
        // resolve through the shared workspace resolver to the barrel, and the BFS must follow the
        // re-export to MyComp's carrier so the provider resolves its props.
        let uri = session.open_fixture_file("src/AppBarrel.vue").await;
        let _mycomp = session.open_fixture_file("src/components/MyComp.vue").await;

        if !session.wait_until_ready(&uri, "{{ count }}", 3, "count").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        let pos = session.find_position(&uri, "<MyComp", 1);
        let hover = session.hover_text(&uri, pos).await;
        assert!(
            hover.is_some(),
            "hover on @/-barrel-imported MyComp should return a result"
        );
        let text = hover.unwrap();
        assert!(
            text.contains("foo"),
            "MyComp imported through a @/-aliased barrel should resolve its props (foo); got: {text}"
        );
    }
);

// ---------------------------------------------------------------------------
// tsconfig references
// ---------------------------------------------------------------------------

real_provider_test!(
    hover_tsconfig_references,
    fixture = "tsconfig-references",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;
        let _mycomp = session.open_fixture_file("src/MyComp.vue").await;

        if !session.wait_until_ready(&uri, "{{ count }}", 3, "count").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        let pos = session.find_position(&uri, "<MyComp", 1);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on MyComp should return a result");
        let text = hover.unwrap();
        assert!(text.contains("foo"), "MyComp hover should mention foo, got: {text}");
    }
);

// ---------------------------------------------------------------------------
// No config: fallback mode
// ---------------------------------------------------------------------------

real_provider_test!(
    hover_no_config,
    fixture = "no-config",
    async fn run(session) {
        let uri = session.open_fixture_file("App.vue").await;

        if !session.wait_until_ready(&uri, "{{ count }}", 3, "count").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        let pos = session.find_position(&uri, "{{ count }}", 3);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on count should return a result");
        let text = hover.unwrap();
        assert!(text.contains("count"), "hover should mention count, got: {text}");
    }
);

// ---------------------------------------------------------------------------
// Single file: minimal fixture
// ---------------------------------------------------------------------------

real_provider_test!(
    hover_single_file,
    fixture = "single-file",
    async fn run(session) {
        let uri = session.open_fixture_file("App.vue").await;

        if !session.wait_until_ready(&uri, "{{ count }}", 3, "count").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        let pos = session.find_position(&uri, "{{ count }}", 3);
        let hover = session.hover_text(&uri, pos).await;
        assert!(hover.is_some(), "hover on count should return a result");
        let text = hover.unwrap();
        assert!(text.contains("count"), "hover should mention count, got: {text}");
    }
);

real_provider_test!(
    completion_single_file,
    fixture = "single-file",
    async fn run(session) {
        let uri = session.open_fixture_file("App.vue").await;

        if !session.wait_until_ready(&uri, "{{ count }}", 3, "count").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        let pos = session.find_position(&uri, "{{ count }}", 3);
        let labels = session.completion_labels(&uri, pos, None).await;
        assert!(labels.contains(&"count".to_string()), "should complete count, got: {labels:?}");
        assert!(labels.contains(&"doubled".to_string()), "should complete doubled, got: {labels:?}");
        assert!(labels.contains(&"increment".to_string()), "should complete increment, got: {labels:?}");
    }
);
