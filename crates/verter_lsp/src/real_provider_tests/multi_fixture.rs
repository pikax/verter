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
// Nested multi-hop @/ barrel: `@/components/layouts` -> layouts/index.ts
// (`export { default as PageContent }`) -> PageContent.vue
// ---------------------------------------------------------------------------
//
// The literal avava user symptom: `import { PageContent } from '@/components/layouts'`
// where the aliased directory index re-exports a `.vue` carrier. This is a 2-hop
// chain (alias resolution + one re-export). The barrel BFS must resolve the `@/`
// specifier through the shared workspace resolver to the `layouts` index, follow
// its `export { default as PageContent }` re-export to PageContent's carrier, and
// sync that carrier so the provider resolves its props.

real_provider_test!(
    hover_nested_aliased_barrel,
    fixture = "barrel-nested-alias",
    async fn run(session) {
        let uri = session.open_fixture_file("src/AppNested.vue").await;
        let _page = session
            .open_fixture_file("src/components/layouts/PageContent.vue")
            .await;

        if !session.wait_until_ready(&uri, "{{ count }}", 3, "count").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        let pos = session.find_position(&uri, "<PageContent", 1);
        let hover = session.hover_text(&uri, pos).await;
        assert!(
            hover.is_some(),
            "hover on @/-nested-barrel PageContent should return a result"
        );
        let text = hover.unwrap();
        assert!(
            text.contains("heading"),
            "PageContent imported through a 2-hop @/ barrel should resolve its props (heading); got: {text}"
        );
    }
);

// ---------------------------------------------------------------------------
// `export *` chain through @/: `@/components` -> components/index.ts
// (`export * from './layouts'`) -> layouts/index.ts -> PageContent.vue
// ---------------------------------------------------------------------------
//
// A 3-hop chain reached via `export *` (a barrel-of-barrels) through an `@/`
// alias, terminating at a `.vue` carrier. The BFS must traverse the `export *`
// re-export hop (not just `export { default as }`) and still reach the terminal
// carrier so its props resolve.

real_provider_test!(
    hover_export_star_aliased_barrel,
    fixture = "barrel-nested-alias",
    async fn run(session) {
        let uri = session.open_fixture_file("src/AppStar.vue").await;
        let _page = session
            .open_fixture_file("src/components/layouts/PageContent.vue")
            .await;

        if !session.wait_until_ready(&uri, "{{ count }}", 3, "count").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        let pos = session.find_position(&uri, "<PageContent", 1);
        let hover = session.hover_text(&uri, pos).await;
        assert!(
            hover.is_some(),
            "hover on @/-export-star-barrel PageContent should return a result"
        );
        let text = hover.unwrap();
        assert!(
            text.contains("heading"),
            "PageContent reached through an `export *` @/ barrel-of-barrels should resolve its props (heading); got: {text}"
        );
    }
);

// ---------------------------------------------------------------------------
// default-import-of-no-default -> TS1192
// ---------------------------------------------------------------------------
//
// A barrel whose ONLY export is a NAMED re-export (`export { default as Widget }`,
// no own default) imported as a DEFAULT import (`import Idx from '@/components'`).
// TS rejects this with TS1192 ("Module has no default export") BEFORE any prop
// check on `<Idx>`. This asserts the merged template+TS diagnostic set surfaces
// the 1192 on the `.vue` carrier (a diagnostics assertion, not a hover one).

real_provider_test!(
    diagnostics_default_import_no_default_ts1192,
    fixture = "barrel-default-import-no-default",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;
        let _widget = session
            .open_fixture_file("src/components/Widget.vue")
            .await;

        // Warm the provider's inferred project (a cold program returns no
        // diagnostics on the first request).
        if !session.wait_until_ready(&uri, "{{ count }}", 3, "count").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

        let diags = session.merged_diagnostics(&uri).await;
        assert!(
            !diags.is_empty(),
            "a default import of a no-default barrel must surface a diagnostic; got none"
        );
        // TS1192 = "Module '...' has no default export". The provider-diagnostic
        // merge carries the TS code as a string (`NumberOrString::String("1192")`);
        // assert by code, falling back to the message text for provider/TS-version
        // drift.
        let has_1192 = diags.iter().any(|d| {
            matches!(
                d.code.as_ref(),
                Some(tower_lsp_server::ls_types::NumberOrString::String(s)) if s == "1192"
            ) || d.message.contains("has no default export")
        });
        assert!(
            has_1192,
            "the default-import-of-no-default barrel must surface TS1192 \
             (\"has no default export\"); got: {diags:?}"
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
