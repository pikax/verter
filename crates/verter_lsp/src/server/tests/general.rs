use super::*;

#[test]
fn resolve_import_path_relative() {
    let result = resolve_import_path("C:/project/src/views", "./Foo.vue");
    assert_eq!(result, "C:/project/src/views/Foo.vue");

    let result = resolve_import_path("C:/project/src/views", "../components/Bar.vue");
    assert_eq!(result, "C:/project/src/components/Bar.vue");
}

#[test]
fn resolve_import_path_alias_returns_raw() {
    // Non-relative imports (aliases) are returned as-is — they need VFS resolution
    let result = resolve_import_path("C:/project/src/views", "@/components/Foo.vue");
    assert_eq!(
        result, "@/components/Foo.vue",
        "alias import should be returned as-is (unresolvable by resolve_import_path)"
    );
    // This means `resolved == target_normalized` will never match for aliases,
    // causing component parents to always be empty for alias-based imports.
}

#[test]
fn vue_tsx_collision_guard_rejects_when_host_missing_source() {
    // The resolver thinks /workspace/src/Real.vue.tsx belongs to the project
    // and strips the suffix to get /workspace/src/Real.vue, but the host
    // has never compiled Real.vue → collision guard must reject.
    let resolver =
        verter_resolution::ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        )]);
    let host = VerterHost::new_standalone(HostConfig::default());
    // Do NOT upsert /workspace/src/Real.vue into host

    assert_eq!(
        source_id_from_provider_carrier_path(&resolver, &host, "/workspace/src/Real.vue.tsx"),
        None,
        ".vue.tsx in project but no backing .vue in host should return None"
    );
}

#[test]
fn svelte_ts_rune_module_resolves_to_itself_not_phantom_component() {
    // A REAL `store.svelte.ts` rune module (non-component carrier) is owned by
    // the project. The resolver strips `.ts` → `store.svelte` (a carrier path),
    // but no backing `store.svelte` component source exists in the host. The
    // generalized collision guard must reject the phantom `store.svelte` and
    // map the rune module to ITSELF.
    let resolver =
        verter_resolution::ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        )]);
    let host = VerterHost::new_standalone(HostConfig::default());
    let rune_language = verter_session::FileLanguage::adapter_module(
        verter_session::ScriptSourceType::Ts,
        verter_session::FrameworkAdapterId::svelte(),
        verter_session::LanguageId::new(verter_session::SVELTE_RUNE_MODULE_LANGUAGE_ID),
    );
    host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/workspace/src/store.svelte.ts".to_string()),
        input_id: "/workspace/src/store.svelte.ts".to_string(),
        source: "export const count = $state(0);\n".into(),
        file_language: rune_language,
        aliases: Vec::new(),
    })
    .unwrap();
    // No `store.svelte` component is upserted — the only real source is the
    // rune module itself.

    let mapped =
        source_id_from_provider_carrier_path(&resolver, &host, "/workspace/src/store.svelte.ts");
    assert_eq!(
        mapped.as_deref(),
        Some("/workspace/src/store.svelte.ts"),
        "a real .svelte.ts rune module with no backing .svelte must map to ITSELF, \
         not the phantom store.svelte component"
    );
    assert_ne!(
        mapped.as_deref(),
        Some("/workspace/src/store.svelte"),
        "must NOT reverse-map to a phantom .svelte component"
    );
}

#[test]
fn svelte_component_virtual_still_resolves_to_carrier() {
    // The genuine component-virtual case: a real `Foo.svelte` component source
    // exists, and its `Foo.svelte.ts` API virtual must still reverse-map to the
    // `Foo.svelte` carrier (the generalization must not break this).
    let resolver =
        verter_resolution::ModuleResolverCore::new(vec![verter_workspace::ide_project_config(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        )]);
    let host = VerterHost::new_standalone(HostConfig::default());
    host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/workspace/src/Foo.svelte".to_string()),
        input_id: "/workspace/src/Foo.svelte".to_string(),
        source: "<script>let x = 1;</script>".into(),
        file_language: verter_session::FileLanguage::svelte(),
        aliases: Vec::new(),
    })
    .unwrap();

    assert_eq!(
        source_id_from_provider_carrier_path(
            &resolver,
            &host,
            "/workspace/src/Foo.svelte.verter.ts"
        )
        .as_deref(),
        Some("/workspace/src/Foo.svelte"),
        "a Foo.svelte.verter.ts API virtual with a backing Foo.svelte component must \
         reverse-map to the carrier (the reserved .verter.ts infix — a bare \
         .svelte.ts is a rune-module path, not the API carrier)"
    );
    assert_eq!(
        source_id_from_provider_carrier_path(&resolver, &host, "/workspace/src/Foo.svelte.tsx")
            .as_deref(),
        Some("/workspace/src/Foo.svelte"),
        "a Foo.svelte.tsx IDE virtual with a backing Foo.svelte component must \
         reverse-map to the carrier"
    );
}

#[test]
fn to_pascal_case_pins_dollar_unicode_and_double_extension_behavior() {
    // Direct, exhaustive pin of the filename→identifier formatter contract.
    // (`build_workspace_components` is the only caller; this isolates the
    // edge cases the helper itself owns.) `to_pascal_case` is in scope via the
    // module's `use self::server_utils::*` glob.

    // Baseline / regression cases.
    assert_eq!(to_pascal_case("my-button"), "MyButton");
    assert_eq!(to_pascal_case("my_comp"), "MyComp");
    assert_eq!(to_pascal_case("MyComponent"), "MyComponent");
    assert_eq!(to_pascal_case("index"), "Index");
    assert_eq!(to_pascal_case("Model.Named"), "ModelNamed");

    // `$` GAP — `$` is a VALID identifier char and is NOT a separator: it is
    // kept verbatim. The doc explicitly excludes `$` from the separator set.
    assert_eq!(
        to_pascal_case("My$Comp"),
        "My$Comp",
        "`$` is a valid identifier char and must be preserved, not split on"
    );
    // A LEADING `$` is a valid identifier start and must survive (NOT prefixed
    // with `_`, NOT dropped). PascalCase capitalizes the first ALPHA char after
    // it; `$` itself has no uppercase form.
    assert_eq!(
        to_pascal_case("$special"),
        "$special",
        "a leading `$` is a valid identifier start and must be preserved verbatim"
    );
    assert_eq!(to_pascal_case("$my-comp"), "$myComp");

    // `.vue.ts` / double-carrier-extension GAP. `strip_carrier_extension`
    // strips ONLY a registered carrier suffix, so a `.vue.ts` sidecar yields the
    // stem `"Model.Named.vue"` (the `.ts` is not a carrier). Such files are
    // EXCLUDED upstream by `is_framework_carrier()`, but if the stem ever
    // reached the formatter it must still produce a VALID identifier — the inner
    // `.vue` is just another `.` separator. No invalid identifier can result.
    let double_ext_stem =
        verter_session_query::resolution::strip_carrier_extension("Model.Named.vue.ts");
    assert_eq!(
        double_ext_stem, "Model.Named.vue.ts",
        "`.ts` is not a carrier extension, so the `.vue.ts` suffix is not stripped"
    );
    assert_eq!(
        to_pascal_case(double_ext_stem),
        "ModelNamedVueTs",
        "every `.` is a separator, so the double-extension stem stays a valid identifier"
    );
    // And on a genuine carrier-stripped stem the result is clean.
    assert_eq!(
        to_pascal_case(verter_session_query::resolution::strip_carrier_extension(
            "Model.Named.vue"
        )),
        "ModelNamed"
    );

    // Non-ASCII GAP — ASCII-only by documented design: non-ASCII chars are
    // separators (dropped). `café` → `Caf` (the `é` splits, nothing follows).
    assert_eq!(
        to_pascal_case("café"),
        "Caf",
        "non-ASCII chars are treated as separators (ASCII-only by design)"
    );
    assert_eq!(to_pascal_case("über-comp"), "BerComp");
    // A stem with NO ASCII identifier chars yields the empty string (the carrier
    // is then skipped upstream rather than emitting a bad name).
    assert_eq!(to_pascal_case("日本語"), "");
    assert_eq!(to_pascal_case("---"), "");

    // The leading-digit guard.
    assert_eq!(to_pascal_case("2cool"), "_2cool");
    assert_eq!(to_pascal_case("3d-view"), "_3dView");
}

#[test]
fn import_resolved_matches_target_exact() {
    assert!(import_resolved_matches_target(
        &verter_session::framework::HostLanguageClassifier::default(),
        "C:/project/src/components/Foo.vue",
        "C:/project/src/components/Foo.vue"
    ));
}

#[test]
fn import_resolved_matches_target_svelte_carrier() {
    // The fuzzy import matcher is carrier-generic: `./Popup` resolves a
    // `.svelte` carrier just as it does `.vue` (gap-5 import resolution).
    assert!(import_resolved_matches_target(
        &verter_session::framework::HostLanguageClassifier::default(),
        "C:/proj/src/Popup",
        "C:/proj/src/Popup.svelte"
    ));
    assert!(import_resolved_matches_target(
        &verter_session::framework::HostLanguageClassifier::default(),
        "C:/proj/src/Popover",
        "C:/proj/src/Popover/index.svelte"
    ));
    assert!(import_resolved_matches_target(
        &verter_session::framework::HostLanguageClassifier::default(),
        "C:/proj/src/Popover",
        "C:/proj/src/Popover/Popover.svelte"
    ));
    // Discrimination: a resolved that already carries a `.svelte` ext gets no
    // fuzzy match (mirrors the `.vue` early-out), and an unrelated target does
    // not match.
    assert!(!import_resolved_matches_target(
        &verter_session::framework::HostLanguageClassifier::default(),
        "C:/proj/src/Popup.svelte",
        "C:/proj/src/Other.svelte"
    ));
    assert!(!import_resolved_matches_target(
        &verter_session::framework::HostLanguageClassifier::default(),
        "C:/proj/src/Popup",
        "C:/proj/src/Other.svelte"
    ));
}

#[test]
fn import_resolved_matches_target_missing_vue_ext() {
    // Import `../Popup` resolves to `C:/proj/src/Popup` (no ext)
    // Target is `C:/proj/src/Popup.vue`
    assert!(import_resolved_matches_target(
        &verter_session::framework::HostLanguageClassifier::default(),
        "C:/proj/src/Popup",
        "C:/proj/src/Popup.vue"
    ));
}

#[test]
fn import_resolved_matches_target_directory_index() {
    // Import `./Popover` resolves to `C:/proj/src/Popover` (directory)
    // Target is `C:/proj/src/Popover/index.vue`
    assert!(import_resolved_matches_target(
        &verter_session::framework::HostLanguageClassifier::default(),
        "C:/proj/src/Popover",
        "C:/proj/src/Popover/index.vue"
    ));
}

#[test]
fn import_resolved_matches_target_directory_same_name() {
    // Import `./Popover` resolves to `C:/proj/src/Popover` (directory)
    // Target is `C:/proj/src/Popover/Popover.vue`
    assert!(import_resolved_matches_target(
        &verter_session::framework::HostLanguageClassifier::default(),
        "C:/proj/src/Popover",
        "C:/proj/src/Popover/Popover.vue"
    ));
}

#[test]
fn import_resolved_does_not_match_different_component() {
    assert!(!import_resolved_matches_target(
        &verter_session::framework::HostLanguageClassifier::default(),
        "C:/proj/src/Popup",
        "C:/proj/src/Dialog.vue"
    ));
    assert!(!import_resolved_matches_target(
        &verter_session::framework::HostLanguageClassifier::default(),
        "C:/proj/src/Popup",
        "C:/proj/src/PopupMenu.vue"
    ));
}

#[test]
fn collect_imported_carrier_priority_ids_keeps_only_resolved_vue_imports() {
    let analysis = verter_session_query::analysis::script_snapshot::ScriptAnalysisSnapshot {
        imports: vec![
            verter_session_query::analysis::types::AnalyzedImport {
                source: "./MyComp.vue".to_string(),
                owner: verter_type_expr::TopLevelOwnerId::instance(0),
                is_type_only: false,
                bindings: Vec::new(),
                span: verter_span::Span::new(0, 0),
                resolved_canonical_id: Some("C:/project/src/MyComp.vue".to_string()),
            },
            verter_session_query::analysis::types::AnalyzedImport {
                source: "./utils".to_string(),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                is_type_only: false,
                bindings: Vec::new(),
                span: verter_span::Span::new(0, 0),
                resolved_canonical_id: Some("C:/project/src/utils.ts".to_string()),
            },
            verter_session_query::analysis::types::AnalyzedImport {
                source: "./Other.vue".to_string(),
                owner: verter_type_expr::TopLevelOwnerId::instance(0),
                is_type_only: false,
                bindings: Vec::new(),
                span: verter_span::Span::new(0, 0),
                resolved_canonical_id: None,
            },
            verter_session_query::analysis::types::AnalyzedImport {
                source: "./MyComp.vue".to_string(),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                is_type_only: false,
                bindings: Vec::new(),
                span: verter_span::Span::new(0, 0),
                resolved_canonical_id: Some("C:/project/src/MyComp.vue".to_string()),
            },
        ],
        module_references: Vec::new(),
        bindings: Vec::new(),
        macros: Vec::new(),
        macro_usage: None,
        macro_type_deps: Vec::new(),
        flags: verter_session_query::analysis::types::AnalysisFlags::empty(),
        exported_functions: Vec::new(),
        vue_api_calls: Vec::new(),
        dom_query_calls: Vec::new(),
        css_var_manipulations: Vec::new(),
        script_binding_occurrences: Vec::new(),
        store_usages: Vec::new(),
        store_definitions: Vec::new(),
        first_await_offset: None,
        type_enhancements: None,
        options_api: None,
        nested_macro_calls: Vec::new(),
        is_typescript: false,
        declaration_entries: Vec::new(),
        style_vbind_roots: Vec::new(),
    };

    let ids = collect_imported_carrier_priority_ids(
        &verter_session::framework::HostLanguageClassifier::default(),
        &analysis,
    );

    assert_eq!(
        ids,
        vec!["C:/project/src/MyComp.vue".to_string()],
        "should keep one resolved .vue canonical id"
    );
    assert!(
        !ids.iter().any(|id| id.ends_with(".ts")),
        "non-Vue imports must be excluded"
    );
}

#[test]
fn collect_imported_carrier_priority_ids_falls_back_to_relative_resolution() {
    let imports = vec![
        verter_session_query::analysis::types::AnalyzedImport {
            source: "./TypedSlotComp.vue".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::instance(0),
            is_type_only: false,
            bindings: Vec::new(),
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: None,
        },
        verter_session_query::analysis::types::AnalyzedImport {
            source: "./utils".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            is_type_only: false,
            bindings: Vec::new(),
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: None,
        },
    ];

    let ids = collect_imported_carrier_priority_ids_from_imports_with_transient_fallback(
        &verter_session::framework::HostLanguageClassifier::default(),
        &imports,
        Some("/workspace/src/TemplateSlotCases.vue"),
        |parent, specifier| {
            if parent == "/workspace/src/TemplateSlotCases.vue"
                && specifier == "./TypedSlotComp.vue"
            {
                Some("/workspace/src/TypedSlotComp.vue".to_string())
            } else if parent == "/workspace/src/TemplateSlotCases.vue" && specifier == "./utils" {
                Some("/workspace/src/utils.ts".to_string())
            } else {
                None
            }
        },
    );

    assert_eq!(
        ids,
        vec!["/workspace/src/TypedSlotComp.vue".to_string()],
        "unresolved direct Vue imports should still be prioritized via relative fallback"
    );
}

#[test]
fn refused_later_carrier_resolution_discards_the_entire_priority_batch() {
    let imports = vec![
        verter_session_query::analysis::types::AnalyzedImport {
            source: "./First.vue".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            is_type_only: false,
            bindings: Vec::new(),
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: Some("/workspace/src/First.vue".to_string()),
        },
        verter_session_query::analysis::types::AnalyzedImport {
            source: "./Second.vue".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            is_type_only: false,
            bindings: Vec::new(),
            span: verter_span::Span::new(0, 0),
            resolved_canonical_id: None,
        },
    ];

    let publication = collect_imported_carrier_priority_ids_from_imports_for_publication(
        &verter_session::framework::HostLanguageClassifier::default(),
        &imports,
        Some("/workspace/src/App.vue"),
        |_parent, _specifier| {
            verter_workspace::ResolutionPublication::refused(
                verter_audit::NonAdmissionReason::ResolutionUntrackedBackend,
            )
        },
    );

    assert!(
        publication.is_err(),
        "a later refusal must discard the earlier admitted carrier instead of publishing a partial batch"
    );
}

#[tokio::test]
async fn dotted_component_auto_import_edit_is_valid_end_to_end() {
    // DISCRIMINATING END-TO-END: drive the REAL synthesis path rather than
    // handing `build_auto_import_edit` a pre-sanitized name. The dotted carrier
    // `Model.Named.vue` goes through `build_workspace_components` (the single
    // source of truth for the binding name) → the synthesized completion `data`
    // → `build_auto_import_edit`, exactly as the live resolve handler does
    // (`nav_features.rs::handle_completion_resolve` reads `data.component_name`).
    //
    // Pre-fix, `build_workspace_components` produced `component_name = "Model.Named"`,
    // so the emitted statement was the syntax error
    // `import Model.Named from './Model.Named.vue'`. This test FAILS on that
    // behavior and PASSES only with the sanitizer in place.
    let child_source = "<script setup lang=\"ts\"></script>\n<template><div /></template>\n";
    // App has an existing `<script setup>` (required anchor) and does NOT yet
    // import the component.
    let parent_source =
        "<script setup lang=\"ts\">\nconst x = 1\n</script>\n<template>\n  <div />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/Model.Named.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let app_canonical = format!("{workspace_id}/src/App.vue");
    let server = service.inner();

    // 1. The REAL enumeration: derive the binding name + import path from disk.
    let ws_components = build_workspace_components(&server.documents.host(), &app_canonical);
    let model = ws_components
        .iter()
        .find(|c| c.import_path.ends_with("Model.Named.vue"))
        .unwrap_or_else(|| {
            panic!(
                "the dotted carrier must be enumerated, got: {:?}",
                ws_components
                    .iter()
                    .map(|c| (&c.name, &c.import_path))
                    .collect::<Vec<_>>()
            )
        });

    // The synthesized binding name must already be the sanitized identifier —
    // this is the value the completion `data.component_name` carries.
    assert_eq!(
        model.name, "ModelNamed",
        "the enumerated binding name must be the sanitized identifier (drives the import)"
    );
    assert!(
        model.import_path.ends_with("Model.Named.vue"),
        "the import PATH must keep the real on-disk `.Named.vue` stem, got: {:?}",
        model.import_path
    );

    // 2. Feed the REAL synthesized name + path through the edit builder, exactly
    //    as the resolve handler does.
    let edit = server
        .build_auto_import_edit(app_uri.as_str(), &model.name, &model.import_path)
        .expect("auto-import edit should be produced for an existing <script setup>");

    assert_eq!(
        edit.new_text.trim_end(),
        format!("import ModelNamed from '{}'", model.import_path),
        "import must use the sanitized identifier with the real module path"
    );
    // Negative: must NOT emit the invalid dotted identifier form (the pre-fix bug).
    assert!(
        !edit.new_text.contains("import Model.Named "),
        "import statement must NOT contain the invalid `Model.Named` identifier, got: {:?}",
        edit.new_text
    );
    // The binding token must be a single valid identifier with no `.`.
    let binding = edit
        .new_text
        .trim_start()
        .strip_prefix("import ")
        .and_then(|s| s.split_whitespace().next())
        .expect("import statement shape");
    assert!(
        !binding.contains('.') && binding == "ModelNamed",
        "the import binding must be the valid identifier `ModelNamed`, got: {binding:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn nested_dir_dotted_component_keeps_real_path_and_sanitized_binding() {
    // GAP: a dotted component in a NESTED directory must keep its correct real
    // relative import PATH while the binding is the sanitized identifier.
    // `src/a/b/Dotted.Name.vue` imported into `src/App.vue` →
    // `import DottedName from './a/b/Dotted.Name.vue'`.
    let child_source = "<script setup lang=\"ts\"></script>\n<template><div /></template>\n";
    let parent_source =
        "<script setup lang=\"ts\">\nconst x = 1\n</script>\n<template>\n  <div />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/a/b/Dotted.Name.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let app_canonical = format!("{workspace_id}/src/App.vue");
    let server = service.inner();

    let ws_components = build_workspace_components(&server.documents.host(), &app_canonical);
    let nested = ws_components
        .iter()
        .find(|c| c.name == "DottedName")
        .unwrap_or_else(|| {
            panic!(
                "nested dotted carrier must enumerate as `DottedName`, got: {:?}",
                ws_components
                    .iter()
                    .map(|c| (&c.name, &c.import_path))
                    .collect::<Vec<_>>()
            )
        });

    // The PATH is never sanitized: it must be the correct nested relative path
    // with the real dotted filename preserved.
    assert_eq!(
        nested.import_path, "./a/b/Dotted.Name.vue",
        "nested dotted carrier must keep its real relative path"
    );

    let edit = server
        .build_auto_import_edit(app_uri.as_str(), &nested.name, &nested.import_path)
        .expect("auto-import edit for nested dotted component");
    assert_eq!(
        edit.new_text.trim_end(),
        "import DottedName from './a/b/Dotted.Name.vue'",
        "nested import must pair the sanitized binding with the real nested path"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_component_attribute_fallback_never_leaks_vue_directives() {
    let child_source =
        "<script lang=\"ts\">\nconst internalOnly = true;\n</script>\n<p>empty</p>\n";
    let zero_prop_source = "<script lang=\"ts\">\nimport EmptyChild from './EmptyChild.svelte';\n</script>\n<EmptyChild ";
    let unresolved_source = "<script lang=\"ts\">\nconst local = true;\n</script>\n<MissingChild ";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/EmptyChild.svelte", "svelte", child_source),
        ("src/App.svelte", "svelte", zero_prop_source),
    ])
    .await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    let server = service.inner();

    for (index, (name, source)) in [
        ("zero-public-prop", zero_prop_source),
        ("unresolved", unresolved_source),
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            let update = server.documents.did_change(&app_uri, 2, source);
            assert!(update.changed, "unresolved fixture must replace the parent");
        }
        let position = LineIndex::new_utf16(source)
            .offset_to_position(source.len() as u32)
            .expect("completion position");
        let labels = completion_labels(
            server
                .completion(completion_params(&app_uri, position, None))
                .await
                .expect("completion request should succeed"),
        );
        assert!(
            labels.iter().all(|label| {
                !label.starts_with("v-")
                    && !matches!(label.as_str(), "@click" | "@input" | "@change")
            }),
            "{name}: Svelte attribute fallback must not leak Vue directives: {labels:?}"
        );
    }

    drain_handle.abort();
    drop(service);
}

#[tokio::test(flavor = "multi_thread")]
async fn did_change_preserves_dependency_receipt_only_when_import_frontier_is_unchanged() {
    for (extension, language_id, first_source, local_edit, import_edit) in [
        (
            "vue",
            "vue",
            "<script setup lang=\"ts\">\nimport FirstChild from './FirstChild.vue'\nconst local = 1\n</script>\n<template><FirstChild />{{ local }}</template>\n",
            "<script setup lang=\"ts\">\nimport FirstChild from './FirstChild.vue'\nconst local = 2\n</script>\n<template><FirstChild />{{ local }}</template>\n",
            "<script setup lang=\"ts\">\nimport SecondChild from './SecondChild.vue'\nconst local = 2\n</script>\n<template><SecondChild />{{ local }}</template>\n",
        ),
        (
            "svelte",
            "svelte",
            "<script lang=\"ts\">\nimport FirstChild from './FirstChild.svelte'\nconst local = 1\n</script>\n<FirstChild />{local}\n",
            "<script lang=\"ts\">\nimport FirstChild from './FirstChild.svelte'\nconst local = 2\n</script>\n<FirstChild />{local}\n",
            "<script lang=\"ts\">\nimport SecondChild from './SecondChild.svelte'\nconst local = 2\n</script>\n<SecondChild />{local}\n",
        ),
    ] {
        let app_path = format!("src/App.{extension}");
        let first_path = format!("src/FirstChild.{extension}");
        let second_path = format!("src/SecondChild.{extension}");
        let (_temp, service, drain_handle, _provider, workspace_id) =
            make_definition_test_server(&[
                (&first_path, language_id, "<template><div /></template>"),
                (&second_path, language_id, "<template><div /></template>"),
                (&app_path, language_id, first_source),
            ])
            .await;
        let server = service.inner();
        let uri = workspace_uri(&workspace_id, &app_path);
        server.ensure_current_file_synced(&uri).await;
        server.publish_import_dependencies_settled(&uri).await;
        assert!(
            server.dependency_readiness_capture(&uri).is_ready(),
            "{extension}: settled receipt precondition"
        );
        super::super::lifecycle::handle_did_change(
            server,
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 2,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: local_edit.to_string(),
                }],
            },
        )
        .await;
        assert!(
            server.dependency_readiness_capture(&uri).is_ready(),
            "{extension}: a local edit with the same import frontier must promote the receipt"
        );

        super::super::lifecycle::handle_did_change(
            server,
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: 3,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: import_edit.to_string(),
                }],
            },
        )
        .await;
        assert!(
            !server.dependency_readiness_capture(&uri).is_ready(),
            "{extension}: changing the import frontier must invalidate the receipt"
        );

        drain_handle.abort();
        drop(service);
    }
}

// =========================================================================
// Unified component contract resolution tests
// =========================================================================

#[tokio::test]
async fn contract_prop_name_navigates_to_child_define_props_field() {
    let child_source =
        "<script setup lang=\"ts\">\ndefineProps<{ title: string; count: number }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <MyComp title=\"hello\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    // Click on "title" in `title="hello"`
    let position = find_document_position(server, &app_uri, "title=\"hello\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("prop should resolve to child");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child component");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "title: string"),
        "definition should point to the child defineProps title field"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_shorthand_prop_returns_both_parent_binding_and_child_field() {
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ bar: string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nconst bar = 'hello'\n</script>\n<template>\n  <MyComp :bar />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    // Click on "bar" in `:bar`
    let position = find_document_position(server, &app_uri, ":bar", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("shorthand prop should resolve");
    let locations = definition_locations(response);

    assert!(
        locations.len() >= 2,
        "shorthand prop should return at least parent binding + child prop, got {}",
        locations.len()
    );
    // One location in parent (the `bar` binding), one in child (the prop field)
    let parent_loc = locations
        .iter()
        .find(|loc| loc.uri == app_uri)
        .expect("should include parent binding location");
    let child_loc = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("should include child prop location");
    assert_eq!(
        parent_loc.range.start.line,
        line_for_snippet(parent_source, "const bar = 'hello'"),
        "parent location should point to the bar binding"
    );
    assert_eq!(
        child_loc.range.start.line,
        line_for_snippet(child_source, "bar: string"),
        "child location should point to the defineProps bar field"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_shorthand_prop_uses_import_target_for_parent_location() {
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ bar: string }>()\n</script>\n";
    let helper_source = "export const bar = 'hello'\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nimport { bar } from './helpers'\n</script>\n<template>\n  <MyComp :bar />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/helpers.ts", "typescript", helper_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let helper_uri = workspace_uri(&workspace_id, "src/helpers.ts");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, ":bar", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("shorthand prop should resolve");
    let locations = definition_locations(response);

    let parent_loc = locations
        .iter()
        .find(|loc| loc.uri == helper_uri)
        .expect("should include imported parent binding target");
    let child_loc = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("should include child prop location");

    assert_eq!(
            parent_loc.range.start.line,
            line_for_snippet(helper_source, "export const bar"),
            "import-backed shorthand should resolve to the imported declaration, not the local import statement"
        );
    assert_eq!(
        child_loc.range.start.line,
        line_for_snippet(child_source, "bar: string"),
        "child location should point to the defineProps bar field"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_shorthand_prop_uses_parent_define_props_field() {
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ bar: string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\ndefineProps<{ bar: string }>()\n</script>\n<template>\n  <MyComp :bar />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, ":bar", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("shorthand prop should resolve");
    let locations = definition_locations(response);

    let parent_loc = locations
        .iter()
        .find(|loc| loc.uri == app_uri)
        .expect("should include parent defineProps field");
    let child_loc = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("should include child prop location");

    assert_eq!(
            parent_loc.range.start.line,
            line_for_snippet(parent_source, "bar: string"),
            "shorthand should resolve to the parent defineProps field when the binding comes from defineProps"
        );
    assert_eq!(
        child_loc.range.start.line,
        line_for_snippet(child_source, "bar: string"),
        "child location should point to the defineProps bar field"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn unresolved_slot_template_does_not_block_later_contract_resolution() {
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ title: string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <UnknownComp>\n    <template #default=\"{ item }\">{{ item }}</template>\n  </UnknownComp>\n  <MyComp title=\"hello\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "title=\"hello\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("later prop contract should still resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child component");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "title: string"),
        "unrelated unresolved slot templates should not prevent later contract resolution"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_event_click_navigates_to_child_define_emits() {
    // This tests that events still work through the unified contract handler
    let child_source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [payload: string] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleCustom(payload: string) {}\n</script>\n<template>\n  <MyComp @custom=\"handleCustom\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("event should resolve to child defineEmits");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "custom: [payload: string]"),
        "event should navigate to child defineEmits field"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_vmodel_named_navigates_to_child_define_model() {
    let child_source =
        "<script setup lang=\"ts\">\nconst title = defineModel<string>('title')\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nconst t = ref('hello')\n</script>\n<template>\n  <MyComp v-model:title=\"t\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    // Cursor on "title" in `v-model:title="t"`
    let position = find_document_position(server, &app_uri, "v-model:title", 8);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("v-model:title should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "defineModel<string>('title')"),
        "v-model:title should navigate to child defineModel('title')"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_vmodel_default_navigates_to_child_define_model() {
    let child_source =
        "<script setup lang=\"ts\">\nconst modelValue = defineModel<string>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nconst val = ref('hello')\n</script>\n<template>\n  <MyComp v-model=\"val\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    // Cursor on "model" in `v-model="val"` — this is the directive name area
    let position = find_document_position(server, &app_uri, "v-model=\"val\"", 3);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("v-model should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "defineModel<string>()"),
        "v-model should navigate to child default defineModel()"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_slot_name_navigates_to_child_define_slots_field() {
    let child_source = "<script setup lang=\"ts\">\ndefineSlots<{ header(props: { title: string }): any }>()\n</script>\n<template>\n  <slot name=\"header\" />\n</template>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <MyComp>\n    <template #header=\"{ title }\">\n      {{ title }}\n    </template>\n  </MyComp>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    // Cursor on "header" in `#header`
    let position = find_document_position(server, &app_uri, "#header", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("slot name should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "header(props:"),
        "slot name should navigate to child defineSlots header field"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_slot_prop_binding_navigates_to_child_slot_binding() {
    let child_source = "<script setup lang=\"ts\">\ndefineSlots<{ default(props: { item: string; index: number }): any }>()\n</script>\n<template>\n  <slot :item=\"row\" :index=\"i\" />\n</template>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <MyComp #default=\"{ item }\">\n    {{ item }}\n  </MyComp>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    // Cursor on "item" inside `#default="{ item }"`
    let position = find_document_position(server, &app_uri, "{ item }", 2);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("slot prop binding should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "item: string"),
        "slot prop binding should navigate to child defineSlots binding"
    );

    drain_handle.abort();
    drop(service);
}

// @ai-generated - Records the known boundary: an imported defineSlots catalog is not
// available to the analyzer-backed hover lookup yet, so the resolved child remains silent.
#[tokio::test]
async fn contract_imported_define_slots_slot_name_without_analyzer_fields_remains_silent() {
    const CHILD_WITH_UNRESOLVED_SLOTS: &str = "<script setup lang=\"ts\">\n\
import type { ImportedSlots } from './slot-types'\n\
defineSlots<ImportedSlots>()\n\
</script>\n";
    const PARENT: &str = "<script setup lang=\"ts\">\n\
import TypedChild from './TypedChild.vue'\n\
</script>\n\
<template>\n\
  <TypedChild>\n\
    <template #header=\"{ title }\">{{ title }}</template>\n\
  </TypedChild>\n\
</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/TypedChild.vue", "vue", CHILD_WITH_UNRESOLVED_SLOTS),
        ("src/App.vue", "vue", PARENT),
    ])
    .await;
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/TypedChild.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "#header", 1);
    let target = {
        let doc = server.documents.get(&app_uri).expect("parent document");
        let analysis = server
            .documents
            .get_analysis(&app_uri)
            .expect("parent analysis");
        let offset = doc
            .line_index
            .position_to_offset(&position)
            .expect("slot offset");
        crate::features::hover::child_hover_target_at_offset(offset, &doc.source, &analysis)
            .expect("#header must enter the native child-slot path")
    };
    let outcome = server
        .child_hover_for_target(&app_uri, &target)
        .expect("child hover resolution should succeed");
    let child_canonical = crate::documents::uri_to_canonical_id(&child_uri);
    let host = server.documents.host();
    let child_analysis = host
        .get_analysis(&child_canonical)
        .expect("the available child must have analysis");
    let (macro_index, slot_macro) = child_analysis
        .macros
        .iter()
        .enumerate()
        .find(|(_, mac)| {
            mac.kind == verter_session_query::analysis::types::AnalyzedMacroKind::DefineSlots
        })
        .expect("the child must have a defineSlots macro");
    let resolver_surface =
        host.resolve_vue_macro_surface(&verter_session::typeinfo::VueMacroSurfaceRequest {
            owner_canonical: std::sync::Arc::from(child_canonical.as_str()),
            macro_index,
            macro_kind: slot_macro.kind,
            // The public resolver re-derives the authoritative current hash.
            root_identity: [0u8; 16],
            level: verter_session::typeinfo::TypeInfoQueryLevel::FullMetadata,
        });
    let actual = server
        .hover(hover_params(&app_uri, position))
        .await
        .expect("hover request should succeed");
    assert!(
        resolver_surface.is_none()
            && matches!(
                &outcome,
                crate::server::component_resolve::ChildHoverOutcome::SurfaceAvailableNoMatch
            )
            && actual.is_none(),
        "the unresolved imported slot catalog remains outside analyzer slot_fields: the shared \
         resolver root is unavailable, but the resolved child classifies as an available no-match \
         and stays silent; resolver_surface={resolver_surface:?}, \
         outcome={outcome:?}, hover={actual:?}"
    );

    drain_handle.abort();
    drop(service);
}

// =========================================================================
// Template-attribute navigation: kebab↔camel, longhand, fail-closed
// =========================================================================

#[tokio::test]
async fn contract_kebab_prop_usage_navigates_to_camel_define_props() {
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ myProp: string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nconst v = 'x'\n</script>\n<template>\n  <MyComp :my-prop=\"v\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, ":my-prop=", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("kebab prop should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "myProp: string"),
        "kebab :my-prop must land on camel myProp declaration"
    );
    assert_ne!(
        (target.range.start.line, target.range.start.character),
        (0, 0),
        "must not fall back to file-start"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_camel_prop_usage_navigates_to_kebab_define_props() {
    let child_source =
        "<script setup lang=\"ts\">\ndefineProps<{ 'my-prop': string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nconst v = 'x'\n</script>\n<template>\n  <MyComp :myProp=\"v\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, ":myProp=", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("camel prop should resolve to kebab declare");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "'my-prop': string"),
        "camel :myProp must land on kebab 'my-prop' declaration"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_longhand_v_bind_prop_navigates_to_child() {
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ title: string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nconst v = 'x'\n</script>\n<template>\n  <MyComp v-bind:title=\"v\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    // Cursor on "title" in longhand v-bind:title
    let position = find_document_position(server, &app_uri, "v-bind:title=", 7);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("longhand v-bind:title should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "title: string"),
        "v-bind:title must land on defineProps title"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_kebab_event_usage_navigates_to_camel_define_emits() {
    let child_source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ myEvent: [v: string] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction onMyEvent(v: string) { void v }\n</script>\n<template>\n  <MyComp @my-event=\"onMyEvent\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@my-event=", 1);

    let t0 = std::time::Instant::now();
    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("kebab event should resolve");
    let first_ms = t0.elapsed().as_millis();
    let t1 = std::time::Instant::now();
    let _ = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("repeat goto definition should succeed");
    let repeat_ms = t1.elapsed().as_millis();
    eprintln!("kebab event def latency first={first_ms}ms repeat={repeat_ms}ms");

    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "myEvent: [v: string]"),
        "kebab @my-event must land on camel myEvent emit"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_longhand_v_on_event_navigates_to_child() {
    let child_source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [v: string] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handle(v: string) { void v }\n</script>\n<template>\n  <MyComp v-on:custom=\"handle\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "v-on:custom=", 5);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("longhand v-on:custom should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "custom: [v: string]"),
        "v-on:custom must land on defineEmits custom"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_kebab_slot_usage_navigates_to_camel_define_slots() {
    let child_source = "<script setup lang=\"ts\">\ndefineSlots<{ mySlot(props: { title: string }): any }>()\n</script>\n<template>\n  <slot name=\"mySlot\" />\n</template>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <MyComp>\n    <template #my-slot=\"{ title }\">\n      {{ title }}\n    </template>\n  </MyComp>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "#my-slot=", 1);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("kebab slot should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "mySlot(props:"),
        "kebab #my-slot must land on camel mySlot declaration"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_longhand_v_slot_navigates_to_child() {
    let child_source = "<script setup lang=\"ts\">\ndefineSlots<{ header(props: { title: string }): any }>()\n</script>\n<template>\n  <slot name=\"header\" />\n</template>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <MyComp>\n    <template v-slot:header=\"{ title }\">\n      {{ title }}\n    </template>\n  </MyComp>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "v-slot:header=", 7);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("longhand v-slot:header should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "header(props:"),
        "v-slot:header must land on defineSlots header"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_directive_v_if_expression_navigates_to_script_binding() {
    let source = "<script setup lang=\"ts\">\nconst showPanel = true\n</script>\n<template>\n  <div v-if=\"showPanel\">x</div>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", source)]).await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "v-if=\"showPanel\"", 6);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("v-if expression identifier should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == app_uri)
        .expect("definition should stay in the same file");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(source, "const showPanel"),
        "v-if showPanel must land on the script declaration"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_directive_v_for_expression_navigates_to_script_binding() {
    let source = "<script setup lang=\"ts\">\nconst items = [1, 2]\n</script>\n<template>\n  <div v-for=\"item in items\" :key=\"item\">{{ item }}</div>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", source)]).await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    // Position on `items` in `v-for="item in items"`
    let position = find_document_position(server, &app_uri, "in items\"", 3);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("v-for expression identifier should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == app_uri)
        .expect("definition should stay in the same file");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(source, "const items"),
        "v-for items must land on the script declaration"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_directive_v_show_expression_navigates_to_script_binding() {
    let source = "<script setup lang=\"ts\">\nconst showPanel = true\n</script>\n<template>\n  <div v-show=\"showPanel\">x</div>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", source)]).await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "v-show=\"showPanel\"", 8);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("v-show expression identifier should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == app_uri)
        .expect("definition should stay in the same file");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(source, "const showPanel"),
        "v-show showPanel must land on the script declaration"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_dynamic_bind_arg_navigates_to_script_binding() {
    // Dynamic-arg form: the `key` in `:[key]="val"` is a computed attribute
    // NAME expression — it must navigate to its script declaration (the value
    // `val` follows the ordinary directive-expression path).
    let source = "<script setup lang=\"ts\">\nconst key = 'title'\nconst val = 'a'\n</script>\n<template>\n  <div :[key]=\"val\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", source)]).await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, ":[key]=\"val\"", 2);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("dynamic-arg identifier should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == app_uri)
        .expect("definition should stay in the same file");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(source, "const key"),
        ":[key] must land on the script key declaration"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_longhand_kebab_v_on_event_navigates_to_camel_emit() {
    // Combined kebab × longhand: `v-on:my-event` → camel `myEvent`.
    let child_source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ myEvent: [v: string] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction onMyEvent(v: string) { void v }\n</script>\n<template>\n  <MyComp v-on:my-event=\"onMyEvent\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "v-on:my-event=", 5);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("longhand kebab event should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "myEvent: [v: string]"),
        "v-on:my-event must land on camel myEvent emit"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_longhand_kebab_v_bind_prop_navigates_to_camel_define_props() {
    // Combined kebab × longhand: `v-bind:my-prop` → camel `myProp`.
    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ myProp: string }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nconst v = 'x'\n</script>\n<template>\n  <MyComp v-bind:my-prop=\"v\" />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "v-bind:my-prop=", 7);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("longhand kebab prop should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "myProp: string"),
        "v-bind:my-prop must land on camel myProp declaration"
    );
    assert_ne!(
        (target.range.start.line, target.range.start.character),
        (0, 0),
        "must not fall back to file-start"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_longhand_kebab_v_slot_navigates_to_camel_define_slots() {
    // Combined kebab × longhand: `v-slot:my-slot` → camel `mySlot`.
    let child_source = "<script setup lang=\"ts\">\ndefineSlots<{ mySlot(props: { title: string }): any }>()\n</script>\n<template>\n  <slot name=\"mySlot\" />\n</template>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <MyComp>\n    <template v-slot:my-slot=\"{ title }\">\n      {{ title }}\n    </template>\n  </MyComp>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", child_source),
        ("src/App.vue", "vue", parent_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "v-slot:my-slot=", 7);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("longhand kebab slot should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == child_uri)
        .expect("definition should point to child");

    assert_eq!(
        target.range.start.line,
        line_for_snippet(child_source, "mySlot(props:"),
        "v-slot:my-slot must land on camel mySlot declaration"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn recognized_inexact_direct_component_contract_miss_is_request_cache_only() {
    const CHILD: &str = "<script setup lang=\"ts\">\ndefineProps<{ title: string; unusedOnly?: boolean }>()\n</script>\n<p>{{ title }}</p>\n";
    const PARENT_INITIAL: &str =
        "<script setup lang=\"ts\">\nimport DraftCard from './DraftCard.vue';\n</script>\n<template><DraftCard /></template>";
    const PARENT: &str =
        "<script setup lang=\"ts\">\nimport DraftCard from './DraftCard.vue';\n</script>\n<template><DraftCard un";
    for kind in [crate::TypeProviderKind::None, crate::TypeProviderKind::Tsgo] {
        let (_temp, service, drain_handle, provider, workspace_id) =
            make_definition_test_server_with_config(
                &[
                    ("src/DraftCard.vue", "vue", CHILD),
                    ("src/App.vue", "vue", PARENT_INITIAL),
                ],
                kind,
                HostConfig {
                    analysis_scope: Some(verter_semantic::analysis::AnalysisScope::BUILD),
                    metrics_enabled: true,
                    ..HostConfig::default()
                },
                false,
            )
            .await;
        let server = service.inner();
        let app_uri = workspace_uri(&workspace_id, "src/App.vue");
        let app_id = format!("{workspace_id}/src/App.vue");
        let child_id = format!("{workspace_id}/src/DraftCard.vue");
        let cursor = PARENT.find("<DraftCard un").expect("component") + "<DraftCard un".len();
        let position = LineIndex::new_utf16(PARENT)
            .offset_to_position(cursor as u32)
            .expect("completion position");

        server.publish_import_dependencies_settled(&app_uri).await;
        assert!(server.cached_child_public_contract(&child_id).is_some());
        assert!(server.documents.did_change(&app_uri, 2, PARENT).changed);

        {
            let document = server.documents.get(&app_uri).expect("open parent");
            let structure = document
                .feature_snapshot
                .as_ref()
                .expect("parent feature snapshot")
                .structure();
            let context =
                crate::documents::carrier_structure::authored_component_attribute_name_context(
                    structure,
                    cursor as u32,
                );
            assert!(
                matches!(
                    context,
                    Some(
                        crate::documents::carrier_structure::AuthoredComponentAttributeNameContext::InexactUnclosedOpening {
                            ref tag,
                        }
                    ) if tag == "DraftCard"
                ),
                "context={context:?}, nodes={:#?}",
                structure.inventory().markup().nodes()
            );
        }
        let ingress = server
            .documents
            .host()
            .get_script_ingress(&app_id)
            .expect("current progressive script ingress");
        assert!(ingress.imports.iter().any(|import| {
            !import.is_type_only
                && import
                    .bindings
                    .iter()
                    .any(|binding| !binding.is_type_only && binding.name == "DraftCard")
        }));

        server.evict_child_public_contract_for_test(&child_id);
        let publication_lane = server.import_sync.lock_for(&app_id);
        let publication_guard = publication_lane.lock().await;

        let metrics_before = server.documents.host().metrics_snapshot();
        let provenance_before = server.documents.host().provenance_snapshot();
        let projection_count = server.child_public_contract_projection_count_for_test();
        let provider_calls = provider.file_sync_calls().len();
        let workspace = server
            .vfs_workspace
            .read()
            .clone()
            .expect("published test workspace");
        let reads_before = workspace.vfs_provenance_snapshot();
        let labels = completion_labels(
            server
                .completion(completion_params(&app_uri, position, None))
                .await
                .expect("completion request"),
        );
        assert!(
            !labels.contains(&"unused-only".to_string()),
            "{kind:?}: a recognized authored direct carrier must fail closed when its committed child contract is absent: {labels:?}"
        );
        let metrics_after = server.documents.host().metrics_snapshot();
        let provenance_after = server.documents.host().provenance_snapshot();
        assert_eq!(
            provenance_after.get_analysis_calls, provenance_before.get_analysis_calls,
            "{kind:?}: completion must not consult child host analysis on a direct contract miss"
        );
        assert_eq!(
            metrics_after.compile_requests, metrics_before.compile_requests,
            "{kind:?}: completion must not compile on the foreground request"
        );
        assert_eq!(
            server.child_public_contract_projection_count_for_test(),
            projection_count,
            "{kind:?}: completion must not compose the missing contract"
        );
        assert_eq!(provider.file_sync_calls().len(), provider_calls);
        let reads_after = workspace.vfs_provenance_snapshot();
        assert_eq!(
            (
                reads_after.native_fs_read_file_miss_count,
                reads_after.native_fs_read_dir_count,
                reads_after.resolution_evidence_live_read_count,
            ),
            (
                reads_before.native_fs_read_file_miss_count,
                reads_before.native_fs_read_dir_count,
                reads_before.resolution_evidence_live_read_count,
            ),
            "{kind:?}: recognized direct contract miss must not perform live workspace/resolution reads"
        );

        drop(publication_guard);
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while server.cached_child_public_contract(&child_id).is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("recognized miss must enqueue bounded background contract repair");
        let restored = completion_labels(
            server
                .completion(completion_params(&app_uri, position, None))
                .await
                .expect("completion after background repair"),
        );
        assert!(
            restored.contains(&"unused-only".to_string()),
            "{kind:?}: background child-contract publication must restore the direct prop: {restored:?}"
        );

        drain_handle.abort();
        drop(service);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn module_augmentation_body_edit_invalidates_child_contract_until_background_republication() {
    const TYPES: &str = "export interface DraftProps { title: string }\n";
    const AUG_V1: &str = "import './types';\ndeclare module './types' { interface DraftProps { fromAug?: boolean } }\n";
    const AUG_V2: &str = "import './types';\ndeclare module './types' { interface DraftProps { changedAug?: boolean } }\n";
    const CHILD: &str = "<script lang=\"ts\">\nimport './aug';\nimport type { DraftProps } from './types';\nlet { title }: DraftProps = $props();\n</script>\n<p>{title}</p>\n";
    const PARENT: &str =
        "<script lang=\"ts\">\nimport DraftCard from './DraftCard.svelte';\n</script>\n<DraftCard un />";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_config(
            &[
                ("src/types.ts", "typescript", TYPES),
                ("src/aug.ts", "typescript", AUG_V1),
                ("src/DraftCard.svelte", "svelte", CHILD),
                ("src/App.svelte", "svelte", PARENT),
            ],
            crate::TypeProviderKind::None,
            HostConfig {
                analysis_scope: Some(verter_semantic::analysis::AnalysisScope::BUILD),
                metrics_enabled: true,
                ..HostConfig::default()
            },
            false,
        )
        .await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    let app_id = format!("{workspace_id}/src/App.svelte");
    let aug_uri = workspace_uri(&workspace_id, "src/aug.ts");
    let child_id = format!("{workspace_id}/src/DraftCard.svelte");
    let cursor = PARENT.find("<DraftCard un").expect("component") + "<DraftCard un".len();
    let position = LineIndex::new_utf16(PARENT)
        .offset_to_position(cursor as u32)
        .expect("completion position");

    server.publish_import_dependencies_settled(&app_uri).await;
    let initial = completion_labels(
        server
            .completion(completion_params(&app_uri, position, None))
            .await
            .expect("initial completion"),
    );
    assert!(
        initial.contains(&"fromAug".to_string()),
        "fixture must prove the loaded child contract consumed the module augmentation: {initial:?}"
    );

    super::super::lifecycle::handle_did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: aug_uri,
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: AUG_V2.to_string(),
            }],
        },
    )
    .await;
    assert!(
        server.cached_child_public_contract(&child_id).is_none(),
        "an augmentation contributor edit must invalidate the child contract witness"
    );
    let publication_lane = server.import_sync.lock_for(&app_id);
    let publication_guard = publication_lane.lock().await;

    let provenance_before = server.documents.host().provenance_snapshot();
    let metrics_before = server.documents.host().metrics_snapshot();
    let projection_before = server.child_public_contract_projection_count_for_test();
    let provider_calls_before = provider.file_sync_calls().len();
    let workspace = server
        .vfs_workspace
        .read()
        .clone()
        .expect("published workspace");
    let reads_before = workspace.vfs_provenance_snapshot();
    let cold = completion_labels(
        server
            .completion(completion_params(&app_uri, position, None))
            .await
            .expect("completion while dependency-backed contract is cold"),
    );
    assert!(!cold.contains(&"fromAug".to_string()));
    assert!(!cold.contains(&"changedAug".to_string()));
    assert_eq!(
        server
            .documents
            .host()
            .provenance_snapshot()
            .get_analysis_calls,
        provenance_before.get_analysis_calls
    );
    assert_eq!(
        server.documents.host().metrics_snapshot().compile_requests,
        metrics_before.compile_requests
    );
    assert_eq!(
        server.child_public_contract_projection_count_for_test(),
        projection_before
    );
    assert_eq!(provider.file_sync_calls().len(), provider_calls_before);
    let reads_after = workspace.vfs_provenance_snapshot();
    assert_eq!(
        (
            reads_after.native_fs_read_file_miss_count,
            reads_after.native_fs_read_dir_count,
            reads_after.resolution_evidence_live_read_count,
        ),
        (
            reads_before.native_fs_read_file_miss_count,
            reads_before.native_fs_read_dir_count,
            reads_before.resolution_evidence_live_read_count,
        )
    );

    drop(publication_guard);
    server.publish_import_dependencies_settled(&app_uri).await;
    assert!(server.cached_child_public_contract(&child_id).is_some());
    let restored = completion_labels(
        server
            .completion(completion_params(&app_uri, position, None))
            .await
            .expect("completion after ambient republish"),
    );
    assert!(
        restored.contains(&"changedAug".to_string()),
        "background publication must restore the changed augmented contract: {restored:?}"
    );
    assert!(!restored.contains(&"fromAug".to_string()));

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn imported_child_contract_with_unchanged_dependency_stays_current_across_parent_edit() {
    const CHILD: &str = "<script lang=\"ts\">\nimport type { Snippet } from 'svelte';\ninterface Props { title: string; unusedOnly?: boolean; children?: Snippet }\nlet { title }: Props = $props();\n</script>\n<p>{title}</p>\n";
    const PARENT: &str = "<script lang=\"ts\">\nimport DraftCard from './DraftCard.svelte';\n</script>\n<p>ready</p>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_config(
            &[
                ("src/DraftCard.svelte", "svelte", CHILD),
                ("src/App.svelte", "svelte", PARENT),
            ],
            crate::TypeProviderKind::None,
            HostConfig {
                analysis_scope: Some(verter_semantic::analysis::AnalysisScope::BUILD),
                metrics_enabled: true,
                ..HostConfig::default()
            },
            false,
        )
        .await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    let child_id = format!("{workspace_id}/src/DraftCard.svelte");

    server.publish_import_dependencies_settled(&app_uri).await;
    assert!(server.cached_child_public_contract(&child_id).is_some());
    let projection_count = server.child_public_contract_projection_count_for_test();
    assert!(provider.file_sync_calls().is_empty());

    let changed = PARENT.replace("<p>ready</p>", "<DraftCard ");
    super::super::lifecycle::handle_did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: app_uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: changed,
            }],
        },
    )
    .await;
    assert!(server.cached_child_public_contract(&child_id).is_some());
    assert!(provider.file_sync_calls().is_empty());
    assert_eq!(
        server.child_public_contract_projection_count_for_test(),
        projection_count,
        "an unrelated parent edit must promote the still-valid exact dependency witness"
    );
    assert!(provider.file_sync_calls().is_empty());

    drain_handle.abort();
    drop(service);
}

#[tokio::test(flavor = "multi_thread")]
async fn barrel_component_route_cache_is_binding_exact_and_provenance_fenced() {
    const ALPHA: &str = "<script lang=\"ts\">\ninterface Props { alphaOnly?: boolean }\nlet {}: Props = $props();\n</script>\n";
    const BETA: &str = "<script lang=\"ts\">\ninterface Props { betaOnly?: boolean }\nlet {}: Props = $props();\n</script>\n";
    const BARREL: &str = "export { default as Alpha } from './Alpha.svelte';\nexport { default as Beta } from './Beta.svelte';\n";
    const PARENT: &str = "<script lang=\"ts\">\nimport { Alpha as LocalAlpha, Beta as LocalBeta } from './components';\n</script>\n<LocalAlpha al />";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_config(
            &[
                ("src/Alpha.svelte", "svelte", ALPHA),
                ("src/Beta.svelte", "svelte", BETA),
                ("src/components.ts", "typescript", BARREL),
                ("src/App.svelte", "svelte", PARENT),
            ],
            crate::TypeProviderKind::None,
            HostConfig {
                analysis_scope: Some(verter_semantic::analysis::AnalysisScope::BUILD),
                metrics_enabled: true,
                ..HostConfig::default()
            },
            false,
        )
        .await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    let app_id = format!("{workspace_id}/src/App.svelte");
    let barrel_uri = workspace_uri(&workspace_id, "src/components.ts");
    let cursor = PARENT.find("<LocalAlpha al").expect("component") + "<LocalAlpha al".len();
    let route_props = |local_binding: &str| {
        let availability = server
            .cached_barrel_component_contract(&app_id, "./components", local_binding)
            .unwrap_or_else(|| panic!("published route for {local_binding}"));
        let verter_session::framework::ComponentContractAvailability::Supported(contract) =
            availability
        else {
            panic!("fixture contract for {local_binding} must be supported")
        };
        contract
            .props
            .iter()
            .map(|prop| prop.name.to_string())
            .collect::<Vec<_>>()
    };

    server.publish_import_dependencies_settled(&app_uri).await;
    assert_eq!(route_props("LocalAlpha"), vec!["alphaOnly"]);
    assert_eq!(route_props("LocalBeta"), vec!["betaOnly"]);
    assert!(provider.file_sync_calls().is_empty());

    // A cache miss on an already-authored barrel binding fails closed without
    // re-entering live resolution/evidence reads from completion.
    server.evict_barrel_component_route_for_test(&app_id, "LocalAlpha");
    let publication_lane = server.import_sync.lock_for(&app_id);
    let publication_guard = publication_lane.lock().await;
    let workspace = server
        .vfs_workspace
        .read()
        .clone()
        .expect("published test workspace");
    let provenance_before = server.documents.host().provenance_snapshot();
    let metrics_before = server.documents.host().metrics_snapshot();
    let projection_before = server.child_public_contract_projection_count_for_test();
    let provider_calls_before = provider.file_sync_calls().len();
    let reads_before = workspace.vfs_provenance_snapshot();
    {
        let doc = server
            .documents
            .get(&app_uri)
            .expect("open parent document");
        let structure = doc
            .feature_snapshot
            .as_ref()
            .map(|snapshot| snapshot.structure().clone())
            .expect("committed parent structure");
        let blocks = crate::documents::carrier_structure::project_carrier_blocks(&structure);
        let context = crate::features::cursor_context::classify_cursor_context_for_language(
            cursor as u32,
            PARENT,
            &blocks,
            None,
            Some(crate::features::cursor_context::CarrierTemplateLanguage::Svelte),
            Some(&structure),
        );
        assert!(
            matches!(
                context,
                crate::features::cursor_context::CursorContext::Template(
                    crate::features::cursor_context::TemplateCursorContext::AttributeName {
                        ref tag_name,
                        is_component: true,
                        ..
                    }
                ) if tag_name == "LocalAlpha"
            ),
            "test must exercise the exact authored attribute ingress branch: {context:?}"
        );
        let ingress = server
            .documents
            .host()
            .get_script_ingress(&app_id)
            .expect("source-stage import ingress");
        assert!(ingress.imports.iter().any(|import| {
            import.source == "./components"
                && import
                    .bindings
                    .iter()
                    .any(|binding| binding.name == "LocalAlpha")
        }));
    }
    let position = LineIndex::new_utf16(PARENT)
        .offset_to_position(cursor as u32)
        .expect("completion position");
    let labels = completion_labels(
        server
            .completion(completion_params(&app_uri, position, None))
            .await
            .expect("completion request"),
    );
    assert!(!labels.contains(&"alphaOnly".to_string()));
    assert_eq!(
        server
            .documents
            .host()
            .provenance_snapshot()
            .get_analysis_calls,
        provenance_before.get_analysis_calls
    );
    assert_eq!(
        server.documents.host().metrics_snapshot().compile_requests,
        metrics_before.compile_requests
    );
    assert_eq!(
        server.child_public_contract_projection_count_for_test(),
        projection_before
    );
    assert_eq!(provider.file_sync_calls().len(), provider_calls_before);
    let reads_after = workspace.vfs_provenance_snapshot();
    assert_eq!(
        (
            reads_after.native_fs_read_file_miss_count,
            reads_after.native_fs_read_dir_count,
            reads_after.resolution_evidence_live_read_count,
        ),
        (
            reads_before.native_fs_read_file_miss_count,
            reads_before.native_fs_read_dir_count,
            reads_before.resolution_evidence_live_read_count,
        ),
        "a barrel-route cache miss must remain a pure committed-cache lookup"
    );
    drop(publication_guard);
    server.ensure_barrel_imports_synced_for_test(&app_uri).await;
    assert_eq!(route_props("LocalAlpha"), vec!["alphaOnly"]);

    // A changed re-export invalidates the route until background publication
    // recomposes it, then the same authored alias points to the new terminal.
    let changed_barrel = BARREL.replace("'./Alpha.svelte'", "'./Beta.svelte'");
    super::super::lifecycle::handle_did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: barrel_uri,
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: changed_barrel,
            }],
        },
    )
    .await;
    assert!(server
        .cached_barrel_component_contract(&app_id, "./components", "LocalAlpha")
        .is_none());
    server.publish_import_dependencies_settled(&app_uri).await;
    assert_eq!(route_props("LocalAlpha"), vec!["betaOnly"]);

    install_test_resolver_for_root(
        server,
        &workspace_id,
        Some(&format!("{workspace_id}/tsconfig.json")),
    );
    assert!(server
        .cached_barrel_component_contract(&app_id, "./components", "LocalAlpha")
        .is_none());
    server.publish_import_dependencies_settled(&app_uri).await;
    assert_eq!(route_props("LocalAlpha"), vec!["betaOnly"]);

    let renamed_parent = PARENT.replace("LocalAlpha", "RenamedAlpha");
    super::super::lifecycle::handle_did_change(
        server,
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: app_uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: renamed_parent,
            }],
        },
    )
    .await;
    assert!(server
        .cached_barrel_component_contract(&app_id, "./components", "LocalAlpha")
        .is_none());
    assert!(server
        .cached_barrel_component_contract(&app_id, "./components", "RenamedAlpha")
        .is_none());
    server.publish_import_dependencies_settled(&app_uri).await;
    assert_eq!(route_props("RenamedAlpha"), vec!["betaOnly"]);

    drain_handle.abort();
    drop(service);
}

#[tokio::test(flavor = "multi_thread")]
async fn tsgo_imported_child_requires_the_committed_direct_ide_surface() {
    const CHILD_V1: &str = "<script lang=\"ts\">\ninterface Props { title: string }\nlet { title }: Props = $props();\n</script>\n<p>{title}</p>\n";
    const CHILD_V2: &str = "<script lang=\"ts\">\ninterface Props { title: string }\nlet { title }: Props = $props();\n</script>\n<section>{title}</section>\n";
    const PARENT: &str = "<script lang=\"ts\">\nimport DraftCard from './DraftCard.svelte';\n</script>\n<DraftCard />\n";

    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/DraftCard.svelte", "svelte", CHILD_V1),
                ("src/App.svelte", "svelte", PARENT),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    let child_uri = workspace_uri(&workspace_id, "src/DraftCard.svelte");
    let child_id = format!("{workspace_id}/src/DraftCard.svelte");
    server.publish_import_dependencies_settled(&app_uri).await;
    assert!(
        server.imported_carrier_already_delivered(&child_id),
        "precondition: managed tsgo directly received the initial IDE and API buffers"
    );
    let committed = server
        .provider_sync_state_for_source(&child_id)
        .expect("initial direct-buffer state");
    let ide_path = committed.ide_path.clone().expect("managed IDE path");

    assert!(server.documents.did_change(&child_uri, 2, CHILD_V2).changed);
    let edited_ide = server
        .documents
        .recompile_and_refresh_mapper(&child_uri)
        .expect("edited IDE surface");
    let revision = server
        .documents
        .snapshot_identity(&child_uri)
        .expect("edited document identity");
    server.record_carrier_ide_snapshot_with_pin(
        Some((&child_uri, &revision)),
        &child_id,
        &ide_path,
        &edited_ide.code,
        edited_ide.source_map.as_deref(),
    );

    assert!(
        !server.imported_carrier_already_delivered(&child_id),
        "a current editor-store surface cannot witness the still-old managed tsgo IDE buffer"
    );
    provider.set_fail_sync_path(&ide_path);
    let outcome = server
        .sync_imported_carrier_api_lightweight(&child_id)
        .await;
    assert!(
        !outcome.is_complete(),
        "a failed direct IDE reopen must keep imported-child publication retryable"
    );
    let after = server
        .provider_sync_state_for_source(&child_id)
        .expect("failed reopen preserves the prior direct-buffer state");
    assert_eq!(
        after.committed_ide_surface, committed.committed_ide_surface,
        "failed direct sync must not authorize the independently-recorded editor surface"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_progressive_prop_ownership_does_not_survive_props_call_replacement() {
    let initial = "<script lang=\"ts\">\nconst providerTarget = 1;\nfunction source(): { staleProp?: () => void } { return {}; }\nlet { staleProp } = $props();\n</script>\n{@render staleProp?.()}\n";
    let changed = initial.replace("$props()", "source()");
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server(&[("src/App.svelte", "svelte", initial)]).await;
    let server = service.inner();
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    let initial_analysis = server
        .documents
        .source_feature_analysis(&app_uri)
        .expect("initial BUILD analysis");
    assert!(
        initial_analysis
            .template
            .as_deref()
            .is_some_and(|template| template
                .prop_definitions
                .iter()
                .any(|prop| prop.name == "staleProp")),
        "precondition: the initial `$props()` call owns the authored prop"
    );

    assert!(server.documents.did_change(&app_uri, 2, &changed).changed);
    assert!(
        server
            .documents
            .get_analysis(&app_uri)
            .and_then(|analysis| analysis.template)
            .is_none_or(|template| template
                .prop_definitions
                .iter()
                .all(|prop| prop.name != "staleProp")),
        "current analysis must not independently re-author the retired prop"
    );
    let current = server.documents.source_feature_analysis(&app_uri);
    assert!(
        current
            .as_ref()
            .and_then(|analysis| analysis.template.as_deref())
            .is_none_or(|template| template
                .prop_definitions
                .iter()
                .all(|prop| prop.name != "staleProp")),
        "replacing the ownership-producing `$props()` call with an ordinary call must retire its \
         progressive prop"
    );

    assert_svelte_same_file_definition(
        server,
        provider.as_ref(),
        &app_uri,
        ("@render staleProp", 8),
        "providerTarget",
    )
    .await;

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_legacy_on_directive_handler_navigates_to_script_binding() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", SVELTE_TS_ON_SOURCE)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_same_file_definition(
        service.inner(),
        &provider,
        &app_uri,
        ("on:click={handlePick}", 10),
        "handlePick",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_js_legacy_on_directive_handler_navigates_to_script_binding() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", SVELTE_JS_ON_SOURCE)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_same_file_definition(
        service.inner(),
        &provider,
        &app_uri,
        ("on:click={handlePick}", 10),
        "handlePick",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_bind_this_value_navigates_to_script_binding() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", SVELTE_TS_BIND_THIS_SOURCE)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_same_file_definition(
        service.inner(),
        &provider,
        &app_uri,
        ("bind:this={boxEl}", 11),
        "boxEl",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_js_bind_this_value_navigates_to_script_binding() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", SVELTE_JS_BIND_THIS_SOURCE)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_same_file_definition(
        service.inner(),
        &provider,
        &app_uri,
        ("bind:this={boxEl}", 11),
        "boxEl",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_use_action_name_navigates_to_script_binding() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", SVELTE_TS_USE_SOURCE)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_same_file_definition(
        service.inner(),
        &provider,
        &app_uri,
        ("use:tooltipAction={'hint'}", 4),
        "tooltipAction",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_js_use_action_name_navigates_to_script_binding() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", SVELTE_JS_USE_SOURCE)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_same_file_definition(
        service.inner(),
        &provider,
        &app_uri,
        ("use:tooltipAction={'hint'}", 4),
        "tooltipAction",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_class_shorthand_identifier_navigates_to_script_binding() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", SVELTE_TS_CLASS_SOURCE)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_same_file_definition(
        service.inner(),
        &provider,
        &app_uri,
        ("class:isActive", 6),
        "isActive",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_js_class_shorthand_identifier_navigates_to_script_binding() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", SVELTE_JS_CLASS_SOURCE)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_same_file_definition(
        service.inner(),
        &provider,
        &app_uri,
        ("class:isActive", 6),
        "isActive",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_style_shorthand_identifier_navigates_to_script_binding() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", SVELTE_TS_STYLE_SOURCE)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_same_file_definition(
        service.inner(),
        &provider,
        &app_uri,
        ("style:accentColor", 6),
        "accentColor",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_js_style_shorthand_identifier_navigates_to_script_binding() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", SVELTE_JS_STYLE_SOURCE)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_same_file_definition(
        service.inner(),
        &provider,
        &app_uri,
        ("style:accentColor", 6),
        "accentColor",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_component_prop_attribute_navigates_to_child_props_member() {
    let (_temp, service, drain_handle, provider, workspace_id) = make_svelte_definition_server(&[
        ("src/Child.svelte", SVELTE_TS_CHILD_SOURCE),
        ("src/App.svelte", SVELTE_TS_PARENT_PROP_SOURCE),
    ])
    .await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_child_prop_definition(
        service.inner(),
        &provider,
        &workspace_id,
        &app_uri,
        ("title={t}", 1),
        "src/Child.svelte",
        "title",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_js_component_prop_attribute_navigates_to_child_props_member() {
    let (_temp, service, drain_handle, provider, workspace_id) = make_svelte_definition_server(&[
        ("src/Child.svelte", SVELTE_JS_CHILD_SOURCE),
        ("src/App.svelte", SVELTE_JS_PARENT_PROP_SOURCE),
    ])
    .await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_child_prop_definition(
        service.inner(),
        &provider,
        &workspace_id,
        &app_uri,
        ("title={t}", 1),
        "src/Child.svelte",
        "title",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_component_bind_prop_name_navigates_to_child_props_member() {
    let (_temp, service, drain_handle, provider, workspace_id) = make_svelte_definition_server(&[
        ("src/Child.svelte", SVELTE_TS_CHILD_SOURCE),
        ("src/App.svelte", SVELTE_TS_PARENT_BIND_SOURCE),
    ])
    .await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_child_prop_definition(
        service.inner(),
        &provider,
        &workspace_id,
        &app_uri,
        ("bind:title={t}", 5),
        "src/Child.svelte",
        "title",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_js_component_bind_prop_name_navigates_to_child_props_member() {
    let (_temp, service, drain_handle, provider, workspace_id) = make_svelte_definition_server(&[
        ("src/Child.svelte", SVELTE_JS_CHILD_SOURCE),
        ("src/App.svelte", SVELTE_JS_PARENT_BIND_SOURCE),
    ])
    .await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_child_prop_definition(
        service.inner(),
        &provider,
        &workspace_id,
        &app_uri,
        ("bind:title={t}", 5),
        "src/Child.svelte",
        "title",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_component_legacy_onevent_prop_navigates_to_child_props_member() {
    // The legacy `onevent` form on a component is a plain callback prop: its
    // name navigates to the child's `$props` member.
    let (_temp, service, drain_handle, provider, workspace_id) = make_svelte_definition_server(&[
        ("src/Child.svelte", SVELTE_TS_CHILD_SOURCE),
        ("src/App.svelte", SVELTE_TS_PARENT_ONEVENT_SOURCE),
    ])
    .await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_child_prop_definition(
        service.inner(),
        &provider,
        &workspace_id,
        &app_uri,
        ("onclick={handlePick}", 2),
        "src/Child.svelte",
        "onclick",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_js_component_legacy_onevent_prop_navigates_to_child_props_member() {
    let (_temp, service, drain_handle, provider, workspace_id) = make_svelte_definition_server(&[
        ("src/Child.svelte", SVELTE_JS_CHILD_SOURCE),
        ("src/App.svelte", SVELTE_JS_PARENT_ONEVENT_SOURCE),
    ])
    .await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_child_prop_definition(
        service.inner(),
        &provider,
        &workspace_id,
        &app_uri,
        ("onclick={handlePick}", 2),
        "src/Child.svelte",
        "onclick",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

// =========================================================================
// Step 4: Barrel-file export symbol clicks → terminal target
// =========================================================================

#[tokio::test]
async fn barrel_export_navigates_to_terminal_vue_component() {
    // Barrel: `export { default as Overlay } from './Overlay.vue'`
    // Clicking on `Overlay` in the barrel should navigate to Overlay.vue
    let overlay_source = "<script setup lang=\"ts\">\nconst visible = ref(false)\n</script>\n<template>\n  <div>Overlay</div>\n</template>\n";
    let barrel_source = "export { default as Overlay } from './Overlay.vue'\nexport { default as Dialog } from './Dialog.vue'\n";
    let dialog_source = "<script setup lang=\"ts\">\nconst open = ref(false)\n</script>\n<template>\n  <div>Dialog</div>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/Overlay.vue", "vue", overlay_source),
        ("src/Dialog.vue", "vue", dialog_source),
        ("src/index.ts", "typescript", barrel_source),
    ])
    .await;

    let barrel_uri = workspace_uri(&workspace_id, "src/index.ts");
    let overlay_uri = workspace_uri(&workspace_id, "src/Overlay.vue");
    let server = service.inner();
    // Cursor on `Overlay` in `export { default as Overlay }`
    let position = find_document_position(server, &barrel_uri, "as Overlay }", 3);

    let response = server
        .goto_definition(goto_definition_params(&barrel_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("barrel export should resolve to terminal");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == overlay_uri)
        .expect("definition should point to Overlay.vue");

    // Should navigate to the component file — the `default` export of a
    // carrier anchors at the file start (no authored source token).
    assert_eq!(
        target.range,
        Range::default(),
        "barrel export should navigate to the terminal Vue component file start"
    );

    // Negative: should NOT stay in barrel file
    assert!(
        !locations.iter().any(|loc| loc.uri == barrel_uri),
        "barrel export should NOT resolve to barrel file itself"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn barrel_multi_level_navigates_to_terminal() {
    // Two-level barrel: index.ts → components.ts → Button.vue
    let button_source = "<script setup lang=\"ts\">\ndefineProps<{ label: string }>()\n</script>\n<template>\n  <button>{{ label }}</button>\n</template>\n";
    let mid_barrel_source = "export { default as Button } from './Button.vue'\n";
    let top_barrel_source = "export { Button } from './components'\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/Button.vue", "vue", button_source),
        ("src/components.ts", "typescript", mid_barrel_source),
        ("src/index.ts", "typescript", top_barrel_source),
    ])
    .await;

    let top_barrel_uri = workspace_uri(&workspace_id, "src/index.ts");
    let button_uri = workspace_uri(&workspace_id, "src/Button.vue");
    let server = service.inner();
    // Cursor on `Button` in `export { Button } from './components'`
    let position = find_document_position(server, &top_barrel_uri, "{ Button }", 2);

    let response = server
        .goto_definition(goto_definition_params(&top_barrel_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("multi-level barrel should resolve to terminal");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == button_uri)
        .expect("definition should point to Button.vue");

    // The `default` export of a carrier anchors at the file start.
    assert_eq!(
        target.range,
        Range::default(),
        "multi-level barrel should navigate to terminal Vue component file start"
    );

    // Negative: should NOT stay in any barrel file
    assert!(
        !locations.iter().any(|loc| loc.uri == top_barrel_uri),
        "should NOT resolve to top barrel itself"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn barrel_import_binding_in_vue_script_navigates_to_terminal() {
    let overlay_source = "<script setup lang=\"ts\">\nconst visible = ref(false)\n</script>\n<template>\n  <div>Overlay</div>\n</template>\n";
    let barrel_source = "export { default as Overlay } from './Overlay.vue'\n";
    let app_source = "<script setup lang=\"ts\">\nimport { Overlay } from './components'\n</script>\n<template>\n  <Overlay />\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/components/Overlay.vue", "vue", overlay_source),
        ("src/components/index.ts", "typescript", barrel_source),
        ("src/App.vue", "vue", app_source),
    ])
    .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let overlay_uri = workspace_uri(&workspace_id, "src/components/Overlay.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "{ Overlay }", 2);

    let response = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("import binding should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == overlay_uri)
        .expect("definition should point to Overlay.vue");

    assert_eq!(
        target.range,
        Range::default(),
        "Vue script import binding should resolve through barrel to the terminal component file start"
    );
    assert!(
        !locations
            .iter()
            .any(|loc| loc.uri.as_str().ends_with("/src/components/index.ts")),
        "Vue script import binding should not stop at the barrel file"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn barrel_aliased_local_side_navigates_to_source() {
    // `export { default as Popup } from './Popup.vue'`
    // Clicking on `default` (the local side) should navigate to Popup.vue too
    let popup_source = "<script setup lang=\"ts\">\nconst shown = ref(true)\n</script>\n<template>\n  <div>Popup</div>\n</template>\n";
    let barrel_source = "export { default as Popup } from './Popup.vue'\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/Popup.vue", "vue", popup_source),
        ("src/index.ts", "typescript", barrel_source),
    ])
    .await;

    let barrel_uri = workspace_uri(&workspace_id, "src/index.ts");
    let popup_uri = workspace_uri(&workspace_id, "src/Popup.vue");
    let server = service.inner();
    // Cursor on `default` in `export { default as Popup }`
    let position = find_document_position(server, &barrel_uri, "{ default", 2);

    let response = server
        .goto_definition(goto_definition_params(&barrel_uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("local side of aliased re-export should resolve");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == popup_uri)
        .expect("definition should point to Popup.vue");

    // The `default` export of a carrier anchors at the file start.
    assert_eq!(
        target.range,
        Range::default(),
        "local side of aliased barrel export should navigate to terminal file start"
    );

    drain_handle.abort();
    drop(service);
}

// =========================================================================
// Step 5: Resolve type-provider barrel locations to terminal declarations
// =========================================================================

#[tokio::test]
async fn resolve_barrel_locations_follows_reexport_to_terminal() {
    // Setup: barrel re-exports a Vue component
    let comp_source = "<script setup lang=\"ts\">\nconst count = ref(0)\n</script>\n<template><div/></template>\n";
    let barrel_source = "export { default as Counter } from './Counter.vue'\n";
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/Counter.vue", "vue", comp_source),
        ("src/index.ts", "typescript", barrel_source),
    ])
    .await;

    let barrel_id = format!("{workspace_id}/src/index.ts");
    let comp_uri = workspace_uri(&workspace_id, "src/Counter.vue");
    let server = service.inner();

    // Simulate a type provider returning a location in the barrel file
    // pointing to the `Counter` export signature (offset 20..27 in barrel source)
    let barrel_source_stored = server.documents.host().get_source(&barrel_id).unwrap();
    let counter_offset = barrel_source_stored
        .find("Counter")
        .expect("Counter in barrel source") as u32;
    let barrel_li = LineIndex::new(&barrel_source_stored, PositionEncodingKind::UTF16);
    let start_pos = barrel_li
        .offset_to_position(counter_offset)
        .expect("start pos");
    let end_pos = barrel_li
        .offset_to_position(counter_offset + 7)
        .expect("end pos");
    let barrel_uri = workspace_uri(&workspace_id, "src/index.ts");

    let input = Some(GotoDefinitionResponse::Scalar(Location {
        uri: barrel_uri.clone(),
        range: Range {
            start: start_pos,
            end: end_pos,
        },
    }));

    let result = server.resolve_barrel_locations(input);
    let locations = definition_locations(result.expect("should resolve"));
    let target = locations
        .iter()
        .find(|loc| loc.uri == comp_uri)
        .expect("should resolve barrel to Counter.vue");

    // Should navigate to the Counter.vue file start (the carrier default
    // export has no authored source token).
    assert_eq!(
        target.range,
        Range::default(),
        "type provider barrel location should resolve to terminal file start"
    );

    // Negative: should NOT stay in barrel
    assert!(
        !locations.iter().any(|loc| loc.uri == barrel_uri),
        "should NOT remain in barrel file"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn resolve_barrel_locations_preserves_non_barrel() {
    // A location that doesn't point to a barrel should pass through unchanged
    let comp_source =
        "<script setup lang=\"ts\">\nconst x = 1\n</script>\n<template><div/></template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", comp_source)]).await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();

    let input = Some(GotoDefinitionResponse::Scalar(Location {
        uri: app_uri.clone(),
        range: Range::default(),
    }));

    let result = server.resolve_barrel_locations(input);
    let locations = definition_locations(result.expect("should pass through"));
    assert_eq!(locations.len(), 1);
    assert_eq!(
        locations[0].uri, app_uri,
        "non-barrel location should pass through unchanged"
    );
    assert_eq!(
        locations[0].range,
        Range::default(),
        "range should be unchanged"
    );

    drain_handle.abort();
    drop(service);
}

/// The reads one request makes of an imported child — its analysis, then its
/// source or registered structure — describe ONE child revision. An edit that
/// lands between those reads never yields the old analysis interpreted through
/// the edited child: the read is taken again at the edited revision, and a child
/// that moves across every read refuses the answer.
#[tokio::test(flavor = "multi_thread")]
async fn child_reads_split_by_an_edit_never_mix_two_child_revisions() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ beforeProp: string }>()\n</script>\n<template><div /></template>\n";
    let edited_child = "<script setup lang=\"ts\">\ndefineProps<{ afterProp: string }>()\n</script>\n<template><div /></template>\n";
    let app_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n\n<template>\n  <MyComp before-prop=\"literal\" />\n</template>\n";
    let child_uri = open_test_vue(server, "/workspace/src/MyComp.vue", child_source);
    let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);
    let position = Position {
        line: 5,
        character: 3,
    };
    let unmoved = hover_text(
        server
            .hover(hover_params(&app_uri, position))
            .await
            .expect("an unmoved hover succeeds"),
    );
    assert!(
        unmoved.contains("beforeProp"),
        "the child answers the hover: {unmoved}"
    );

    // One edit between the child reads: the answer, if any, describes the
    // edited child alone.
    let edits = Arc::new(AtomicUsize::new(0));
    {
        let server = server.clone();
        let child_uri = child_uri.clone();
        let edits = Arc::clone(&edits);
        server
            .clone()
            .set_child_read_hook_for_test(Some(Box::new(move || {
                if edits.fetch_add(1, Ordering::SeqCst) == 0 {
                    assert!(
                        server
                            .documents
                            .did_change(&child_uri, 2, edited_child)
                            .changed
                    );
                }
            })));
    }
    let split_once = server.hover(hover_params(&app_uri, position)).await;
    server.set_child_read_hook_for_test(None);
    assert!(
        edits.load(Ordering::SeqCst) >= 1,
        "the edit landed between child reads"
    );
    match &split_once {
        Ok(hover) => {
            let text = hover_text(hover.clone());
            assert!(
                text.contains("afterProp") && !text.contains("beforeProp"),
                "an answer read across an edit describes only the edited child: {text}"
            );
        }
        Err(error) => assert_eq!(
            error.code,
            tower_lsp_server::jsonrpc::ErrorCode::ContentModified,
            "a request whose child moved is refused, never failed otherwise"
        ),
    }

    // The child moves across every read: the request is refused.
    let edits = Arc::new(AtomicUsize::new(0));
    {
        let server = server.clone();
        let child_uri = child_uri.clone();
        let edits = Arc::clone(&edits);
        server
            .clone()
            .set_child_read_hook_for_test(Some(Box::new(move || {
                let edit = edits.fetch_add(1, Ordering::SeqCst);
                let text = if edit.is_multiple_of(2) {
                    child_source
                } else {
                    edited_child
                };
                assert!(
                    server
                        .documents
                        .did_change(&child_uri, 3 + edit as i32, text)
                        .changed
                );
            })));
    }
    let always_split = server.hover(hover_params(&app_uri, position)).await;
    server.set_child_read_hook_for_test(None);
    assert!(edits.load(Ordering::SeqCst) >= 2);
    assert!(
        matches!(&always_split, Err(error) if error.code == tower_lsp_server::jsonrpc::ErrorCode::ContentModified),
        "a child that moves across every read refuses the answer: {always_split:?}"
    );
}

/// W02 concurrency bound: N callers that observe the same repair sequence
/// still run exactly one compile. The first attempt advances the sequence
/// under the lane; queued callers join it, while a later request may retry.
///
/// Measured on `compile_cold_runs`, the host's own count of cold compiles
/// STARTED.
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_requests_on_a_broken_revision_compile_at_most_once() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_with_kind(type_provider, crate::TypeProviderKind::Tsgo);
    let server = service.inner();
    install_test_resolver(server);
    server
        .hover_native_semantics_enabled
        .store(false, std::sync::atomic::Ordering::Release);

    let canonical_id = "/workspace/src/App.vue";
    let uri = open_test_vue(
        server,
        canonical_id,
        "<script setup lang=\"ts\" src=\"./missing.ts\"></script>\n<template><div/></template>\n",
    );
    assert!(
        server.documents.get_projection(&uri).is_none(),
        "precondition: unavailable input must leave the carrier projection-less"
    );
    assert!(
        server.current_file_needs_inline_type_provider_sync(&uri),
        "unavailable input is not memoized, so the concurrent cohort is admitted"
    );
    let cold_before = server
        .documents
        .host()
        .provenance_snapshot()
        .compile_cold_runs;
    // B2 rejects missing external block content before compiler admission.
    const COLD_RUNS_PER_ADMITTED_REPAIR: u64 = 0;

    // Hold all four callers after they capture the repair sequence and before
    // any can acquire the lane. This makes the concurrent cohort structural,
    // independent of executor polling order.
    let pauses: Vec<_> = (0..4)
        .map(|_| server.pause_next_ide_sync_after_lease(canonical_id))
        .collect();

    let position = Position {
        line: 0,
        character: 1,
    };
    let requests = futures_util::future::join_all((0..4).map(|_| async {
        let hover = server
            .hover(hover_params(&uri, position))
            .await
            .expect("hover request must not error to the client");
        assert!(hover.is_none(), "a broken carrier fails closed");
    }));
    let release_cohort = async {
        for (arrived, _) in &pauses {
            arrived.notified().await;
        }
        for (_, release) in &pauses {
            release.notify_one();
        }
    };
    let (_results, ()) = futures_util::future::join(requests, release_cohort).await;

    let cold_after = server
        .documents
        .host()
        .provenance_snapshot()
        .compile_cold_runs;
    assert_eq!(
        cold_after - cold_before,
        COLD_RUNS_PER_ADMITTED_REPAIR,
        "external-content deferral must reject before any cold compile"
    );
    let hover_calls = provider
        .calls()
        .iter()
        .filter(|call| matches!(call, MockCall::GetHover { .. }))
        .count();
    assert_eq!(
        hover_calls, 0,
        "a projection-less broken carrier must never reach the provider"
    );
}

/// W02 source-identity fence: the repair retains the response its own compile
/// returned, so every consumer of that response must first prove the carrier
/// source has not moved underneath it.
///
/// The ordering this reproduces is the whole defect: the repair compiles
/// revision A and holds A's provider bytes + mapper, it AWAITS (the carrier
/// gateway, then the provider sync), a `didChange` commits revision B while it
/// is parked, and it then syncs and RECORDS. `record_carrier_ide_surface`
/// resolves the carrier source from the LIVE open document
/// (`resolve_carrier_source`), so an unfenced record pairs A's bytes and A's
/// map with source B — and the recorded pair then PASSES the source-hash
/// validation `capture_provider_request_surface` runs, because both halves of
/// that comparison are B. A later request maps positions in B through A's map:
/// not a transient miss, a genuine MIS-MAPPING, which the Carrier IDE TS
/// Surface Principle forbids outright.
///
/// The fence discards the retained response instead and fails the request
/// closed, exactly as `provider_recovery`'s retry fence does for the same
/// hazard on the query side.
#[tokio::test(flavor = "multi_thread")]
async fn a_repair_whose_carrier_source_moved_mid_flight_records_nothing() {
    let (service, _provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();
    let canonical_id = "/workspace/src/App.vue";

    // Revision B: different bytes, and a source that compiles cleanly — so a
    // failed compile can never be what this test observes.
    const SOURCE_B: &str = r#"<script setup lang="ts">
const msg = 'edited-into-a-longer-and-quite-different-string'
const extra = 42
</script>
<template><div>{{ msg }}{{ extra }}</div></template>
"#;

    let ide_path = server
        .active_ide_path_for_uri(&uri)
        .expect("the baseline sync must commit a live IDE path");
    let baseline = server
        .documents
        .provider_surfaces()
        .current_snapshot(&ide_path)
        .expect("the baseline sync records a CarrierIde surface");
    assert_eq!(
        baseline.carrier_source.as_ref(),
        REQUEST_SURFACE_APP,
        "precondition: the baseline surface describes revision A"
    );

    // Force a repair, and park it between its own compile and every consumer
    // of the response that compile returned.
    server.needs_ide_sync.insert(canonical_id.to_string());
    let (arrived, release) = server.pause_next_ide_sync_after_recompile(canonical_id);

    let repair = server.ensure_current_file_synced(&uri);
    let edit = async {
        arrived.notified().await;
        // The repair now holds revision A's response and has installed
        // nothing, synced nothing, recorded nothing. Commit revision B.
        let _ = server.documents.did_change(&uri, 2, SOURCE_B);
        assert_eq!(
            server
                .documents
                .get(&uri)
                .expect("document stays open")
                .source
                .as_ref(),
            SOURCE_B,
            "the interleaved edit must really have committed revision B, or \
             this test exercises no supersession at all"
        );
        release.notify_one();
    };
    futures_util::future::join(repair, edit).await;

    // NOTHING may pair revision A's provider bytes with revision B's source.
    if let Some(recorded) = server
        .documents
        .provider_surfaces()
        .current_snapshot(&ide_path)
    {
        assert_eq!(
            recorded.carrier_source.as_ref(),
            REQUEST_SURFACE_APP,
            "a surface recorded from the retained response must still describe \
             the revision it was compiled from. Recording revision A's provider \
             bytes and map against the newly committed revision B is the \
             mis-mapping: the pair then validates (both sides are B) and a later \
             request maps B's positions through A's map"
        );
        assert_eq!(
            recorded.provider_content.as_ref(),
            baseline.provider_content.as_ref(),
            "and the surface that survives is the pre-edit one, unchanged — the \
             superseded repair published no new generation"
        );
    }

    // The request-facing consequence: the live source is B and no surface for
    // B has been synced, so the capture fails CLOSED. Under the unfenced
    // record it succeeds — with A's content and A's mapper.
    assert!(
        server.capture_provider_request_surface(&uri).is_none(),
        "a carrier whose live source has no synced surface must fail the \
         capture closed, never hand out a stale mapper that validates"
    );
}

/// Recording is a second write point: the retained identity must be compared
/// while installing the surface, so an edit between the old check and the
/// record cannot pair A's provider bytes/map with B's live source.
#[tokio::test(flavor = "multi_thread")]
async fn a_source_change_after_the_check_prevents_the_surface_record() {
    let (service, provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();
    let canonical_id = "/workspace/src/App.vue";
    const SOURCE_B: &str = r#"<script setup lang="ts">
const msg = 'surface-record-window'
const extra = 42
</script>
<template><div>{{ msg }}{{ extra }}</div></template>
"#;

    let ide_path = server
        .active_ide_path_for_uri(&uri)
        .expect("baseline IDE path");
    let baseline = server
        .documents
        .provider_surfaces()
        .current_snapshot(&ide_path)
        .expect("baseline CarrierIde surface");

    // A restarted engine no longer holds the delivered bytes, so the repair
    // owes the IDE leg and runs it through to the record.
    provider.forget_applied_content();
    server.needs_ide_sync.insert(canonical_id.to_string());
    let (arrived, release) = server.pause_next_ide_sync_before_surface_record(canonical_id);
    let repair = server.ensure_current_file_synced(&uri);
    let edit = async {
        arrived.notified().await;
        let _ = server.documents.did_change(&uri, 2, SOURCE_B);
        release.notify_one();
    };
    futures_util::future::join(repair, edit).await;

    let recorded = server
        .documents
        .provider_surfaces()
        .current_snapshot(&ide_path)
        .expect("the baseline surface remains");
    assert_eq!(
        recorded.carrier_source.as_ref(),
        REQUEST_SURFACE_APP,
        "the record write must retain the baseline revision A surface, never \
         install A's bytes and map under revision B's live source"
    );
    assert_eq!(
        recorded.provider_content.as_ref(),
        baseline.provider_content.as_ref(),
        "the superseded repair must publish no new surface generation"
    );
    assert!(
        server.capture_provider_request_surface(&uri).is_none(),
        "revision B has no synced surface yet and must fail closed"
    );
}

/// The per-leg basis re-check the API and commit fences call must see an edit
/// that lands after the pin was captured. A pin-less transaction started while
/// its document was closed and is current only while it stays closed.
#[tokio::test(flavor = "multi_thread")]
async fn the_api_leg_pin_recheck_follows_the_live_revision() {
    let (service, _provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();
    let canonical_id = "/workspace/src/App.vue";
    const SOURCE_B: &str = r#"<script setup lang="ts">
const msg = 'api-leg-pin'
const extra = 42
</script>
<template><div>{{ msg }}{{ extra }}</div></template>
"#;

    let (pin_uri, pin_revision) = server.documents.open_compile_pin(canonical_id);
    let pin_uri = pin_uri.expect("the fixture's document is open");
    let pin_revision = pin_revision.expect("the open document has a revision");
    assert!(
        server.open_pin_is_current(canonical_id, Some((&pin_uri, &pin_revision))),
        "a pin captured for the live revision is current"
    );

    let _ = server.documents.did_change(&uri, 2, SOURCE_B);
    assert!(
        !server.open_pin_is_current(canonical_id, Some((&pin_uri, &pin_revision))),
        "the API leg's fence must see the edit that landed after its pin — \
         without it a `.d.ts` built from the previous revision would be \
         delivered and committed as this document's public API"
    );
    assert!(
        server.open_pin_is_current("/workspace/src/Closed.vue", None),
        "a closed source has no pin that can have moved, so its leg proceeds"
    );
    assert!(
        !server.open_pin_is_current(canonical_id, None),
        "a transaction that started while its document was closed is refused \
         at delivery once the document is open — its disk-compiled bytes must \
         not land beneath the open document's own transaction"
    );
}

/// The unresolved-preserve helper owns its own provider await and surface
/// record, so it must carry the same retained revision through that await and
/// use the identity-fenced record choke point.
#[tokio::test(flavor = "multi_thread")]
async fn unresolved_preserve_rechecks_retained_identity_at_the_record_write() {
    let (service, _provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();
    let canonical_id = "/workspace/src/App.vue";
    const SOURCE_B: &str = r#"<script setup lang="ts">
const msg = 'unresolved-record-window'
const extra = 42
</script>
<template><div>{{ msg }}{{ extra }}</div></template>
"#;

    let revision = server
        .documents
        .snapshot_identity(&uri)
        .expect("open revision A");
    let ide = server
        .documents
        .get_ide(&uri)
        .expect("revision A IDE output");
    let ide_path = server
        .active_ide_path_for_uri(&uri)
        .expect("baseline IDE path");
    let baseline = server
        .documents
        .provider_surfaces()
        .current_snapshot(&ide_path)
        .expect("baseline CarrierIde surface");
    let (arrived, release) = server.pause_next_ide_sync_before_surface_record(canonical_id);

    let preserve = server.preserve_open_unresolved_carrier(
        canonical_id,
        false,
        Some(&ide.code),
        Some((&uri, &revision)),
    );
    let edit = async {
        arrived.notified().await;
        let _ = server.documents.did_change(&uri, 2, SOURCE_B);
        release.notify_one();
    };
    futures_util::future::join(preserve, edit).await;

    let recorded = server
        .documents
        .provider_surfaces()
        .current_snapshot(&ide_path)
        .expect("the baseline surface remains");
    assert_eq!(
        recorded.carrier_source.as_ref(),
        REQUEST_SURFACE_APP,
        "the unresolved helper must not pair retained A output with live B"
    );
    assert_eq!(
        recorded.provider_content.as_ref(),
        baseline.provider_content.as_ref(),
        "the superseded unresolved preserve must publish no new surface"
    );
}

/// A dirty flag cannot override a deterministic verdict for the exact bytes.
#[tokio::test(flavor = "multi_thread")]
async fn dirty_flag_does_not_readmit_a_deterministic_content_verdict() {
    let provider: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service_with_kind(provider, crate::TypeProviderKind::Tsgo);
    let server = service.inner();
    install_test_resolver(server);

    let canonical_id = "/workspace/src/App.vue";
    let uri = open_test_vue(
        server,
        canonical_id,
        "<script setup lang=\"ts\">\nconst broken = (((\n",
    );
    assert!(
        server.documents.get_projection(&uri).is_none(),
        "precondition: the malformed open must leave the carrier projection-less"
    );
    // The registry-only open above does not mark the server dirty flag.
    // Reconstruct the production state explicitly.
    server.needs_ide_sync.insert(canonical_id.to_string());
    assert!(
        !server.current_file_needs_inline_type_provider_sync(&uri),
        "the exact syntax-verdict bytes remain declined despite a dirty flag"
    );

    // A genuinely NEW revision still owes its one attempt.
    let _ =
        server
            .documents
            .did_change(&uri, 2, "<script setup lang=\"ts\">\nconst broken = ((((\n");
    assert!(
        server.current_file_needs_inline_type_provider_sync(&uri),
        "new content remains repairable"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn drain_owner_loss_retracts_carrier_membership_from_the_ledger() {
    // Gap (a) — production-path coverage for the background-drain no-owner branch.
    // `drain_pending_snapshot_provider_sync` →
    // `sync_pending_snapshot_provider_file` → `sync_pending_carrier_provider_file`'s
    // no-owner arm MUST route owner loss through the membership reconciler so a
    // previously-advertised carrier is RETRACTED from the ledger-backed
    // `getExternalFiles`. DISCRIMINATION: a drain arm that handled only the provider
    // buffer (the pre-fix gap, where the store/ledger membership was never retracted
    // on a drained owner loss) leaves the source advertised — the final assertion
    // catches it. RED is reproduced by removing the `reconcile_membership` call from
    // that no-owner arm.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let source = "/workspace/src/Gone.vue";
    let uri: Uri = "file:///workspace/src/Gone.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div /></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    // A real coordinator over a real on-disk backend (the same store the plugin
    // reads); its membership ledger is the advertisement authority under test.
    let backend = Arc::new(crate::external_ts::TsserverEngineBackend::with_default_host_version());
    let coordinator = crate::external_ts::CarrierPublishCoordinator::new(
        Arc::clone(&backend),
        Arc::clone(&type_provider),
        "5.9.0",
    );

    // Pre-advertise the carrier in the ledger — the state owner loss must retract.
    let ledger = backend.membership_ledger();
    let canonical = crate::external_ts::CanonicalSource::from(source);
    ledger
        .commit(
            &canonical,
            crate::external_ts::MembershipRecord::Advertised {
                project: crate::external_ts::ProjectUri::from("/workspace/tsconfig.json"),
                companions: vec![crate::external_ts::LedgerCompanion {
                    provider_uri: Arc::from("/workspace/src/Gone.vue.tsx"),
                    role: verter_session::external_ts::SnapshotRole::CarrierIde,
                    script_kind: verter_session::external_ts::ScriptKind::Tsx,
                }],
                lease: ledger.current_session(),
            },
        )
        .expect("seed the prior advertisement");
    assert!(
        ledger.is_advertised(&canonical),
        "the carrier must start advertised so the drain has something to retract"
    );

    // A READY snapshot rooted ELSEWHERE: ownership is resolved (ownership_ready) but
    // does NOT own this source → authoritative owner loss (not a cold bootstrap).
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let vfs_workspace = crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/other_ws",
        Some("/other_ws/tsconfig.json"),
    );
    let provider_sync_states = DashMap::new();
    let pending_snapshot_provider_sync = DashSet::new();
    pending_snapshot_provider_sync.insert(source.to_string());

    drain_pending_snapshot_provider_sync(
        Some(&sync),
        &documents,
        &vfs_workspace,
        &provider_sync_states,
        &pending_snapshot_provider_sync,
        false,
        None,
        Some(&coordinator),
        &crate::external_ts::CarrierTransactionCoordinator::new(),
    )
    .await;

    assert!(
        !ledger.is_advertised(&canonical),
        "owner loss through the background drain MUST retract the carrier from the \
         ledger-backed getExternalFiles (the no-owner drain arm routes through the reconciler)"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn drain_owner_transition_partial_failure_retains_stale_path_of_failed_kind() {
    // FIX-2: on an owner transition where the IDE path genuinely changes
    // (.jsx→.tsx), the API sync succeeds but the new IDE `.tsx` sync FAILS.
    // The drain must NOT close the old live `.jsx` (its kind did not sync) and
    // must NOT leave committed state pointing at the unsynced `.tsx`. Pre-fix
    // `synced_any` was true (API synced) → it committed `.tsx` and closed the
    // genuinely-stale `.jsx`, losing the file's only live TSX.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div>{{ msg }}</div></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    // Fail ONLY the new IDE `.tsx` sync; the API `.ts` sync succeeds.
    provider.set_fail_sync_path("/workspace/src/App.vue.tsx");
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    // New owner at `/workspace` → IDE `.tsx`, API `.ts`.
    let vfs_workspace = crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/workspace",
        Some("/workspace/tsconfig.app.json"),
    );

    let provider_sync_states = DashMap::new();
    // Prior owner-aware state: IDE `.jsx` (DIFFERENT → genuinely stale on the
    // owner change), API `.ts` (same), both live.
    provider_sync_states.insert(
        "/workspace/src/App.vue".to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/old/tsconfig.json".to_string(),
            ),
            ide_path: Some("/workspace/src/App.vue.jsx".to_string()),
            api_path: Some("/workspace/src/App.vue.verter.ts".to_string()),
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: true,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );
    let pending_snapshot_provider_sync = DashSet::new();
    pending_snapshot_provider_sync.insert("/workspace/src/App.vue".to_string());

    drain_pending_snapshot_provider_sync(
        Some(&sync),
        &documents,
        &vfs_workspace,
        &provider_sync_states,
        &pending_snapshot_provider_sync,
        false,
        None,
        None,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
    )
    .await;

    let calls = provider.file_sync_calls();
    // Discriminator: the stale `.jsx` IDE path must NOT be closed because its
    // replacement `.tsx` did not sync. (Pre-fix: closed because synced_any.)
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == "/workspace/src/App.vue.jsx"
        )),
        "stale IDE `.jsx` must NOT close when the new `.tsx` sync failed, calls={calls:?}"
    );

    let state = provider_sync_states
        .get("/workspace/src/App.vue")
        .map(|entry| entry.clone())
        .expect("partial transition must retain a committed state");
    // Discriminator: committed IDE path must NOT be the unsynced `.tsx`; it must
    // revert to the previous live `.jsx`.
    assert_eq!(
        state.ide_path.as_deref(),
        Some("/workspace/src/App.vue.jsx"),
        "failed IDE kind must keep the previous live `.jsx`, not the unsynced `.tsx`, got {:?}",
        state.ide_path
    );
    assert!(
        state.ide_background_loaded,
        "the retained `.jsx` IDE path keeps its loaded flag"
    );
    // Positive: the API kind that DID sync advanced + is marked loaded.
    assert_eq!(
        state.api_path.as_deref(),
        Some("/workspace/src/App.vue.verter.ts"),
        "the synced API path is committed"
    );
    assert!(
        state.api_background_loaded,
        "the synced API path is marked loaded"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tsgo_barrel_receipt_does_not_truncate_a_deep_reexport_chain() {
    const HOPS: usize = 12;
    let mut owned_files = Vec::<(String, String, String)>::new();
    owned_files.push((
        "src/Terminal.vue".into(),
        "vue".into(),
        "<script setup lang=\"ts\">\ndefineProps<{ value: string }>()\n</script>\n".into(),
    ));
    for hop in 0..HOPS {
        let target = if hop + 1 == HOPS {
            "./Terminal.vue".to_string()
        } else {
            format!("./barrel{}", hop + 1)
        };
        owned_files.push((
            format!("src/barrel{hop}.ts"),
            "typescript".into(),
            format!("export {{ default as Terminal }} from '{target}'\n"),
        ));
    }
    owned_files.push((
        "src/Usage.vue".into(),
        "vue".into(),
        "<script setup lang=\"ts\">\nimport { Terminal } from './barrel0'\n</script>\n<template><Terminal /></template>\n".into(),
    ));
    let borrowed_files: Vec<(&str, &str, &str)> = owned_files
        .iter()
        .map(|(path, language, source)| (path.as_str(), language.as_str(), source.as_str()))
        .collect();
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(&borrowed_files, crate::TypeProviderKind::Tsgo).await;
    let usage_uri = workspace_uri(&workspace_id, "src/Usage.vue");
    provider.clear_calls();

    service
        .inner()
        .ensure_barrel_imports_synced_for_test(&usage_uri)
        .await;

    assert!(
        provider.file_sync_calls().iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } if path.replace('\\', "/").ends_with("/src/Terminal.vue.tsx")
                || path.replace('\\', "/").ends_with("/src/Terminal.vue")
        )),
        "the terminal carrier beyond the former depth cap must be delivered before readiness"
    );
    drain_handle.abort();
    drop(service);
}

#[tokio::test(flavor = "multi_thread")]
async fn tsserver_barrel_resolution_skips_rewrites_when_ts_extensions_are_allowed() {
    // Official tsserver framework plugins keep ordinary TS/JS modules under
    // TypeScript's disk authority and resolve framework carriers through their
    // store membership. Background publication still walks the barrel to
    // publish the terminal child contract, but must not send rewritten barrel
    // buffers to the provider.
    let foo = "<script setup lang=\"ts\">\ndefineProps<{ foo: boolean }>()\n</script>\n";
    let mid_barrel = "export { default as Foo } from './Foo.vue'\n";
    let top_barrel = "export * from './Foo'\n";
    let usage = "<script setup lang=\"ts\">\nimport { Foo } from './components'\n</script>\n<template><Foo /></template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/components/Foo/Foo.vue", "vue", foo),
                ("src/components/Foo/index.ts", "typescript", mid_barrel),
                ("src/components/index.ts", "typescript", top_barrel),
                ("src/Usage.vue", "vue", usage),
            ],
            crate::TypeProviderKind::Tsserver,
        )
        .await;

    let server = service.inner();
    let usage_uri = workspace_uri(&workspace_id, "src/Usage.vue");
    install_test_resolver_for_root_with_options(
        server,
        &workspace_id,
        Some(&format!("{workspace_id}/tsconfig.json")),
        verter_session_query::resolution::IdeProjectCompilerOptions {
            allow_importing_ts_extensions: true,
            ..Default::default()
        },
    );
    provider.clear_calls();

    server
        .ensure_barrel_imports_synced_for_test(&usage_uri)
        .await;

    let calls = provider.calls();
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::OpenFileBackground { path, .. }
                | MockCall::LoadFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                if path.replace('\\', "/").ends_with("/src/components/index.ts")
                    || path.replace('\\', "/").ends_with("/src/components/Foo/index.ts")
        )),
        "a project that allows TS extension imports must keep authored carrier specifiers and publish no rewritten barrel buffers; calls={calls:?}"
    );

    drain_handle.abort();
    drop(service);
}

#[tokio::test(flavor = "multi_thread")]
async fn tsserver_barrel_resolution_rewrites_carrier_exports_without_ts_extension_permission() {
    let foo = "<script setup lang=\"ts\">\ndefineProps<{ foo: boolean }>()\n</script>\n";
    let mid_barrel = "export { default as Foo } from './Foo.vue'\n";
    let top_barrel = "export * from './Foo'\n";
    let usage = "<script setup lang=\"ts\">\nimport { Foo } from './components'\n</script>\n<template><Foo /></template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/components/Foo/Foo.vue", "vue", foo),
                ("src/components/Foo/index.ts", "typescript", mid_barrel),
                ("src/components/index.ts", "typescript", top_barrel),
                ("src/Usage.vue", "vue", usage),
            ],
            crate::TypeProviderKind::Tsserver,
        )
        .await;

    let usage_uri = workspace_uri(&workspace_id, "src/Usage.vue");
    provider.clear_calls();
    service
        .inner()
        .ensure_barrel_imports_synced_for_test(&usage_uri)
        .await;

    let rewritten = provider
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            MockCall::OpenFile { path, content }
            | MockCall::OpenFileBackground { path, content }
            | MockCall::LoadFile { path, content }
            | MockCall::UpdateFile { path, content } => Some((path, content)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rewritten.len(),
        1,
        "only the barrel whose carrier specifier changes should be published: {rewritten:?}"
    );
    assert!(
        rewritten[0]
            .0
            .replace('\\', "/")
            .ends_with("/src/components/Foo/index.ts")
            && rewritten[0].1.contains("'./Foo.vue.verter.ts'"),
        "the compatibility buffer must target the carrier API surface: {rewritten:?}"
    );

    drain_handle.abort();
    drop(service);
}

/// Public-boundary acceptance: an unused declared prop, event, and slot each
/// publish ONE Verter-owned diagnostic by default (no lint config), faded via
/// `Unnecessary`, anchored on the authored member name; used members and the
/// self-consumed `defineModel` pair stay silent.
#[test]
fn unused_declared_props_emits_slots_surface_by_default_with_unnecessary_tag() {
    let dir = tempfile::tempdir().unwrap();
    let source = "<script setup lang=\"ts\">\n\
                  const props = defineProps<{ used: string; unusedProp: number }>();\n\
                  const emit = defineEmits<{ save: []; unusedEvent: [] }>();\n\
                  defineSlots<{ header(): unknown; unusedSlot(): unknown }>();\n\
                  const title = defineModel<string>('title');\n\
                  console.log(props.used, title.value);\n\
                  emit('save');\n\
                  </script>\n\
                  \n\
                  <template>\n\
                  <div v-if=\"props.used\"><slot name=\"header\" /></div>\n\
                  </template>\n";
    let file = dir.path().join("Comp.vue");
    std::fs::write(&file, source).unwrap();

    let host = crate::test_utils::make_filesystem_test_host(dir.path());
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri =
        crate::uri::path_to_file_uri(&file.to_string_lossy().replace('\\', "/")).expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let cached_verter_diags = Arc::new(DashMap::new());
    let diags =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);

    let by_code = |code: &str| {
        diags
            .iter()
            .filter(|diag| {
                matches!(
                    diag.code.as_ref(),
                    Some(NumberOrString::String(c)) if c == code
                )
            })
            .collect::<Vec<_>>()
    };

    // ── Unused prop: exactly one, faded, on the authored member name ──
    let unused_props = by_code("verter/no-unused-props");
    assert_eq!(
        unused_props.len(),
        1,
        "unused declared prop must surface by default, got: {diags:?}"
    );
    assert!(unused_props[0].message.contains("unusedProp"));
    assert_eq!(
        unused_props[0].tags,
        Some(vec![DiagnosticTag::UNNECESSARY]),
        "editors need the Unnecessary tag for the faded TS-unused look"
    );
    assert_eq!(
        unused_props[0].range.start,
        ascii_position(source, "unusedProp"),
        "diagnostic anchors on the authored declaration member"
    );

    // ── Unused event ──
    let unused_emits = by_code("verter/no-unused-emit-declarations");
    assert_eq!(unused_emits.len(), 1, "got: {diags:?}");
    assert!(unused_emits[0].message.contains("unusedEvent"));
    assert_eq!(unused_emits[0].tags, Some(vec![DiagnosticTag::UNNECESSARY]));
    assert_eq!(
        unused_emits[0].range.start,
        ascii_position(source, "unusedEvent")
    );

    // ── Unused slot ──
    let unused_slots = by_code("verter/no-unused-slots");
    assert_eq!(unused_slots.len(), 1, "got: {diags:?}");
    assert!(unused_slots[0].message.contains("unusedSlot"));
    assert_eq!(unused_slots[0].tags, Some(vec![DiagnosticTag::UNNECESSARY]));
    assert_eq!(
        unused_slots[0].range.start,
        ascii_position(source, "unusedSlot")
    );

    // ── Negatives: used members and the defineModel pair are never flagged ──
    for never_flagged in ["'used'", "'save'", "'header'", "'title'", "update:title"] {
        assert!(
            !diags
                .iter()
                .any(|diag| diag.message.contains(never_flagged)),
            "{never_flagged} is used/self-consumed and must not be flagged: {diags:?}"
        );
    }
}

/// Props-root binding through `withDefaults`: the analyzer stores the root
/// binding on the OUTER `WithDefaults` macro while the INNER `DefineProps`
/// carries the prop fields with no binding of its own. Acceptance: a prop
/// read in `<script setup>` (`props.title`) and a prop read in the template
/// (`{{ props.count }}`) must NOT be flagged.
#[test]
fn with_defaults_bound_props_script_and_template_reads_are_not_flagged() {
    let dir = tempfile::tempdir().unwrap();
    let source = "<script setup lang=\"ts\">\n\
                  const props = withDefaults(defineProps<{ title: string; count: number }>(), { title: 'x', count: 0 });\n\
                  console.log(props.title);\n\
                  </script>\n\
                  \n\
                  <template>\n\
                  <div>{{ props.count }}</div>\n\
                  </template>\n";
    let file = dir.path().join("WithDefaultsUsed.vue");
    std::fs::write(&file, source).unwrap();

    let host = crate::test_utils::make_filesystem_test_host(dir.path());
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri =
        crate::uri::path_to_file_uri(&file.to_string_lossy().replace('\\', "/")).expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let cached_verter_diags = Arc::new(DashMap::new());
    let diags =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);

    assert!(
        !diags.iter().any(|diag| matches!(
            diag.code.as_ref(),
            Some(NumberOrString::String(code)) if code == "verter/no-unused-props"
        )),
        "props read via the bound withDefaults root (script `props.title`, template \
         `{{ props.count }}`) must NOT be flagged, got: {diags:?}"
    );
}

/// The discriminator: under bound `withDefaults` a GENUINELY unused prop must
/// STILL be reported — the fix arms the analysis, it must not silence it.
#[test]
fn with_defaults_bound_props_genuinely_unused_prop_is_still_flagged() {
    let dir = tempfile::tempdir().unwrap();
    let source = "<script setup lang=\"ts\">\n\
                  const props = withDefaults(defineProps<{ title: string; unusedProp: number }>(), { title: 'x', unusedProp: 0 });\n\
                  console.log(props.title);\n\
                  </script>\n\
                  \n\
                  <template>\n\
                  <div />\n\
                  </template>\n";
    let file = dir.path().join("WithDefaultsUnused.vue");
    std::fs::write(&file, source).unwrap();

    let host = crate::test_utils::make_filesystem_test_host(dir.path());
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri =
        crate::uri::path_to_file_uri(&file.to_string_lossy().replace('\\', "/")).expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let cached_verter_diags = Arc::new(DashMap::new());
    let diags =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);

    let unused_props = diags
        .iter()
        .filter(|diag| {
            matches!(
                diag.code.as_ref(),
                Some(NumberOrString::String(code)) if code == "verter/no-unused-props"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        unused_props.len(),
        1,
        "a genuinely-unused prop under bound withDefaults must STILL surface exactly \
         one diagnostic, got: {diags:?}"
    );
    assert!(unused_props[0].message.contains("unusedProp"));
    assert!(
        !unused_props
            .iter()
            .any(|diag| diag.message.contains("title")),
        "the read member must stay silent, got: {diags:?}"
    );
}

/// The escape machinery must now be ARMED: spreading the bound `withDefaults`
/// root whole (`{ ...props }`) is a whole-object escape and suppresses every
/// unused-prop diagnostic (fail-open).
#[test]
fn with_defaults_bound_props_whole_object_spread_suppresses() {
    let dir = tempfile::tempdir().unwrap();
    let source = "<script setup lang=\"ts\">\n\
                  const props = withDefaults(defineProps<{ title: string }>(), { title: 'x' });\n\
                  const all = { ...props };\n\
                  console.log(all);\n\
                  </script>\n\
                  \n\
                  <template>\n\
                  <div />\n\
                  </template>\n";
    let file = dir.path().join("WithDefaultsSpread.vue");
    std::fs::write(&file, source).unwrap();

    let host = crate::test_utils::make_filesystem_test_host(dir.path());
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri =
        crate::uri::path_to_file_uri(&file.to_string_lossy().replace('\\', "/")).expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let cached_verter_diags = Arc::new(DashMap::new());
    let diags =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);

    assert!(
        !diags.iter().any(|diag| matches!(
            diag.code.as_ref(),
            Some(NumberOrString::String(code)) if code == "verter/no-unused-props"
        )),
        "whole-object spread of the bound withDefaults root must suppress (fail-open), \
         got: {diags:?}"
    );
}

/// Template whole-object binding under bound `withDefaults`: `<div
/// v-bind="props">` is an unconsumed occurrence of the bound root — a
/// whole-object escape that suppresses every unused-prop diagnostic
/// (fail-open). The escape is kind-wide by design, so the anti-silencing
/// control is the paired file opened in the same host below: the SAME
/// component without the escape must still flag its genuinely-unused props.
#[test]
fn with_defaults_bound_props_template_whole_object_bind_suppresses() {
    let dir = tempfile::tempdir().unwrap();
    let source = "<script setup lang=\"ts\">\n\
                  const props = withDefaults(defineProps<{ title: string; unusedProp: number }>(), { title: 'x', unusedProp: 0 });\n\
                  </script>\n\
                  \n\
                  <template>\n\
                  <div v-bind=\"props\" />\n\
                  </template>\n";
    let file = dir.path().join("WithDefaultsTemplateBind.vue");
    std::fs::write(&file, source).unwrap();

    let host = crate::test_utils::make_filesystem_test_host(dir.path());
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri =
        crate::uri::path_to_file_uri(&file.to_string_lossy().replace('\\', "/")).expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let cached_verter_diags = Arc::new(DashMap::new());
    let diags =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);

    assert!(
        !diags.iter().any(|diag| matches!(
            diag.code.as_ref(),
            Some(NumberOrString::String(code)) if code == "verter/no-unused-props"
        )),
        "template whole-object `v-bind=\"props\"` on the bound withDefaults root is an \
         escape and must suppress every unused-prop diagnostic, got: {diags:?}"
    );

    // Anti-silencing control: the SAME component without the escape — no prop
    // is read anywhere, so BOTH must surface. Proves the zero above is the
    // escape, not a silent diagnostic pipeline.
    let control_source = "<script setup lang=\"ts\">\n\
                  const props = withDefaults(defineProps<{ title: string; unusedProp: number }>(), { title: 'x', unusedProp: 0 });\n\
                  </script>\n\
                  \n\
                  <template>\n\
                  <div />\n\
                  </template>\n";
    let control_file = dir.path().join("WithDefaultsTemplateBindControl.vue");
    std::fs::write(&control_file, control_source).unwrap();
    let control_uri =
        crate::uri::path_to_file_uri(&control_file.to_string_lossy().replace('\\', "/"))
            .expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: control_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: control_source.to_string(),
    });
    let control_diags = crate::server::document_diagnostics_for_test(
        &documents,
        &control_uri,
        &cached_verter_diags,
        None,
    );
    let control_unused: Vec<_> = control_diags
        .iter()
        .filter(|diag| {
            matches!(
                diag.code.as_ref(),
                Some(NumberOrString::String(code)) if code == "verter/no-unused-props"
            )
        })
        .collect();
    assert_eq!(
        control_unused.len(),
        2,
        "anti-silencing control: without the escape both genuinely-unused props must \
         STILL be flagged, got: {control_diags:?}"
    );
    assert!(
        control_unused.iter().any(|d| d.message.contains("title"))
            && control_unused
                .iter()
                .any(|d| d.message.contains("unusedProp")),
        "control must flag exactly `title` and `unusedProp`, got: {control_diags:?}"
    );
}

/// The standard template-emit pattern — calling the `defineEmits` return
/// binding from a template handler (`@click="emit('close')"`) — is a live
/// emit. The whole emit kind fails open on any template occurrence of the
/// binding (per-name template extraction stays deferred), so NOTHING may be
/// flagged.
#[test]
fn template_emit_binding_call_never_flags_the_emitted_event() {
    let dir = tempfile::tempdir().unwrap();
    let source = "<script setup lang=\"ts\">\n\
                  const emit = defineEmits<{ close: []; other: [] }>();\n\
                  </script>\n\
                  \n\
                  <template>\n\
                  <button @click=\"emit('close')\">x</button>\n\
                  </template>\n";
    let file = dir.path().join("TemplateEmit.vue");
    std::fs::write(&file, source).unwrap();

    let host = crate::test_utils::make_filesystem_test_host(dir.path());
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri =
        crate::uri::path_to_file_uri(&file.to_string_lossy().replace('\\', "/")).expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let cached_verter_diags = Arc::new(DashMap::new());
    let diags =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);

    assert!(
        !diags.iter().any(|diag| matches!(
            diag.code.as_ref(),
            Some(NumberOrString::String(code)) if code == "verter/no-unused-emit-declarations"
        )),
        "a template call through the emit binding must suppress unused-emit \
         diagnostics (fail-open on the binding occurrence), got: {diags:?}"
    );
}

/// A mid-edit BROKEN `<script>` (not just a broken template expression) must
/// fail open: OXC error recovery can silently swallow real usages (here the
/// unterminated template literal eats the `props.color` read and the
/// `emit('save')` call), so no unused-declaration hint may be emitted while
/// the script does not parse.
#[test]
fn broken_script_mid_edit_emits_no_unused_declaration_hints() {
    // Two distinct mid-edit breakage classes: an unterminated template
    // literal (a fatal parse) and an unterminated block comment (a
    // RECOVERABLE parse error) — both swallow the trailing real usages, so
    // both must fail open.
    let sources = [
        (
            "BrokenLiteral.vue",
            "<script setup lang=\"ts\">\n\
             const props = defineProps<{ color: string }>();\n\
             const emit = defineEmits<{ save: [] }>();\n\
             defineSlots<{ header(): unknown }>();\n\
             const wip = `unterminated\n\
             console.log(props.color);\n\
             emit('save');\n\
             </script>\n\
             \n\
             <template>\n\
             <div><slot name=\"header\" /></div>\n\
             </template>\n",
        ),
        (
            "BrokenComment.vue",
            "<script setup lang=\"ts\">\n\
             const props = defineProps<{ color: string }>();\n\
             const emit = defineEmits<{ save: [] }>();\n\
             defineSlots<{ header(): unknown }>();\n\
             /* unterminated mid-edit comment\n\
             console.log(props.color);\n\
             emit('save');\n\
             </script>\n\
             \n\
             <template>\n\
             <div><slot name=\"header\" /></div>\n\
             </template>\n",
        ),
    ];
    for (name, source) in sources {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(name);
        std::fs::write(&file, source).unwrap();

        let host = crate::test_utils::make_filesystem_test_host(dir.path());
        let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
        let uri =
            crate::uri::path_to_file_uri(&file.to_string_lossy().replace('\\', "/")).expect("uri");
        let _ = documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: source.to_string(),
        });

        let cached_verter_diags = Arc::new(DashMap::new());
        let diags = crate::server::document_diagnostics_for_test(
            &documents,
            &uri,
            &cached_verter_diags,
            None,
        );

        assert!(
            !diags.iter().any(|diag| matches!(
                diag.code.as_ref(),
                Some(NumberOrString::String(code)) if code.starts_with("verter/no-unused-")
            )),
            "{name}: a script with parse errors must produce ZERO unused-declaration \
             diagnostics (fail-open — recovery can hide real usages), got: {diags:?}"
        );
    }
}

/// A prop consumed ONLY through `<style>` `v-bind(color)` by BARE name (no
/// `props.` prefix, non-destructured `defineProps`) is live at runtime — the
/// style fact must keep that member alive while a genuinely dead prop in the
/// same component still surfaces (per-member fact, not whole-kind
/// suppression).
#[test]
fn style_vbind_bare_prop_name_keeps_prop_live_and_dead_prop_flagged() {
    let dir = tempfile::tempdir().unwrap();
    let source = "<script setup lang=\"ts\">\n\
                  defineProps<{ color: string; deadProp: string }>();\n\
                  </script>\n\
                  \n\
                  <template>\n\
                  <div class=\"x\">t</div>\n\
                  </template>\n\
                  \n\
                  <style>\n\
                  .x { color: v-bind(color); }\n\
                  </style>\n";
    let file = dir.path().join("StyleVBind.vue");
    std::fs::write(&file, source).unwrap();

    let host = crate::test_utils::make_filesystem_test_host(dir.path());
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri =
        crate::uri::path_to_file_uri(&file.to_string_lossy().replace('\\', "/")).expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let cached_verter_diags = Arc::new(DashMap::new());
    let diags =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);

    let unused_props: Vec<_> = diags
        .iter()
        .filter(|diag| {
            matches!(
                diag.code.as_ref(),
                Some(NumberOrString::String(code)) if code == "verter/no-unused-props"
            )
        })
        .collect();
    assert!(
        !unused_props
            .iter()
            .any(|diag| diag.message.contains("color")),
        "`color` is live through `<style>` v-bind(color) and must not be \
         flagged, got: {unused_props:?}"
    );
    assert!(
        unused_props
            .iter()
            .any(|diag| diag.message.contains("deadProp")),
        "`deadProp` is genuinely unused — the style fact is per-member, not a \
         whole-kind suppression, got: {diags:?}"
    );
}

#[test]
fn test_carrier_language_for() {
    let classifier = verter_session::framework::HostLanguageClassifier::default();
    assert_eq!(
        carrier_language_for(&classifier, "file:///project/src/App.vue"),
        Some(verter_session::FileLanguage::vue())
    );
    assert_eq!(
        carrier_language_for(&classifier, "C:/project/src/App.vue"),
        Some(verter_session::FileLanguage::vue())
    );
    // `.svelte` is a KNOWN carrier row (no carrier implementation is
    // registered behind it — watched events stay inert, requests
    // surface the typed unsupported-language error).
    assert_eq!(
        carrier_language_for(&classifier, "file:///project/src/Box.svelte"),
        Some(verter_session::FileLanguage::svelte())
    );
    assert!(carrier_language_for(&classifier, "file:///project/src/utils.ts").is_none());
    assert!(carrier_language_for(&classifier, "file:///project/tsconfig.json").is_none());
    assert!(carrier_language_for(&classifier, "file:///project/vue.config.js").is_none());
}

/// A host narrowed to Vue classifies every protocol-level carrier decision
/// under its own admission: a `.svelte` path is not a carrier (did-open
/// dependency tracking, watched-file carrier resync and drop edits skip it),
/// a `.svelte.ts` rune module is not an adapter module, and both serve the
/// provider as the plain scripts the host upserted them as — never as the
/// Svelte vertical the operator excluded.
#[test]
fn narrowed_host_protocol_classification_follows_its_admission() {
    let host = VerterHost::new_standalone(HostConfig {
        framework: verter_session::framework::FrameworkOptions::admitting_names(["vue"])
            .expect("vue is composed"),
        ..HostConfig::default()
    });
    let classifier = host.language_classifier();
    assert_eq!(
        carrier_language_for(classifier, "file:///project/src/App.vue"),
        Some(verter_session::FileLanguage::vue())
    );
    assert_eq!(
        carrier_language_for(classifier, "file:///project/src/Box.svelte"),
        None
    );
    assert!(
        !super::super::server_utils::is_default_export_component_carrier(
            classifier,
            "/project/src/Box.svelte"
        )
    );
    assert_eq!(
        super::super::server_utils::adapter_module_language_for(
            classifier,
            "/project/src/store.svelte.ts"
        ),
        None
    );
    assert_eq!(
        super::super::server_utils::self_file_language_for(
            classifier,
            "/project/src/store.svelte.ts"
        ),
        Some(verter_session::FileLanguage::script(
            verter_session::ScriptSourceType::Ts
        ))
    );
    assert_eq!(
        super::super::server_utils::self_file_language_for(classifier, "/project/README.md"),
        None,
        "an unregistered extension still serves no provider buffer"
    );
    assert_eq!(
        super::super::server_utils::self_file_language_for(classifier, "/project/src/Box.svelte"),
        None,
        "an unadmitted carrier serves no provider buffer, like an unregistered extension"
    );
    assert_eq!(
        super::super::server_utils::self_file_language_for(
            classifier,
            "file:///project/src/Box.svelte"
        ),
        None
    );
}

/// A REAL Vue carrier whose IDE context is MISSING (stale / not committed) is a
/// hard error on completion-resolve — never a dropped-edit success.
///
/// Contract: `resolve_provider_auto_import_edits` returns `Ok(None)` ONLY for the
/// no-carrier / self-file cases. A path that DOES reverse-map to a `.vue`
/// carrier (not self-file) but has no live IDE context (no committed provider
/// sync state → `ide_context_by_path` is `None`) is a genuine carrier-resolve
/// FAILURE: it must return `Err`, so the caller reports a structured resolve
/// error rather than silently dropping the provider's non-empty auto-import
/// edits (which recreates "accepted completion but no import").
///
/// Discriminating: the carrier reverse-map is PRESENT (the `.vue` is open +
/// owned), the projection is NOT self-file, the edit set is NON-EMPTY, and only
/// the IDE context is absent. The pre-fix code returned `Ok(None)` on that
/// missing-context branch (dropping the edits); this asserts `Err`, so it FAILS
/// on the pre-fix success and PASSES once the branch fails closed.
#[tokio::test]
async fn missing_ide_context_for_real_carrier_fails_resolve_not_drops_edits() {
    use crate::type_provider::auto_import::ProviderImportEdit;

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let host_for_server = Arc::clone(&host);
    let type_provider_for_server = Arc::clone(&type_provider);
    let (service, _socket) = tower_lsp_server::LspService::new(move |_client| {
        VerterLanguageServer::new(
            crate::outbound::Outbound::default(),
            LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&type_provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsserver,
                type_provider_topology: crate::TypeProviderTopology::ProjectTsserver,
                mcp_port: None,
                type_provider_reason: None,
                type_provider_advisory: None,
                suppress_imported_carrier_prewarm: false,
            },
        )
    });
    let socket = service.inner().outbound().wire();
    let drain = tokio::spawn(async move {
        let mut socket = socket;
        while socket.next().await.is_some() {}
    });
    let server = service.inner();
    install_test_resolver(server);

    // Open a real `.vue` carrier: the host now has the `.vue` source and the
    // canonical↔uri mapping, so the carrier IDE path reverse-maps. We do NOT
    // establish a committed provider sync state (`ensure_current_file_synced`),
    // so `active_ide_path_for_uri` is None ⇒ `ide_context_by_path` is None: the
    // IDE-context-missing branch the fix must fail closed on.
    let vue_uri: Uri = open_test_vue(
        server,
        "/workspace/App.vue",
        "<script setup lang=\"ts\">\nconst x = 1;\n</script>\n<template>{{ x }}</template>",
    );
    let tsx_path = "/workspace/App.vue.tsx";

    // Precondition 1: the carrier reverse-map IS present (real carrier).
    assert_eq!(
        server.carrier_uri_from_ide_path(tsx_path).as_ref(),
        Some(&vue_uri),
        "precondition: the carrier IDE path reverse-maps to the open `.vue` URI"
    );
    // Precondition 2: it is NOT a self-file projection (so the `Ok(None)`
    // self-file branch is not what fires).
    assert!(
        !server.is_self_file_projection(&vue_uri),
        "precondition: a `.vue` carrier is not a self-file projection"
    );
    // Precondition 3: the request surface IS missing (no committed sync state).
    assert!(
        server.capture_provider_request_surface(&vue_uri).is_none(),
        "precondition: with no committed provider sync state the carrier has no \
         capturable request surface"
    );

    // A NON-EMPTY auto-import edit set — the only case the carrier re-anchor
    // runs for. These MUST be placed or the resolve must fail; they may never be
    // silently dropped.
    let provider_edits = vec![ProviderImportEdit {
        start: 0,
        end: 0,
        new_text: "import { foo } from './foo';\n".to_string(),
    }];
    let resolved =
        super::super::nav_features_completion_resolve::resolve_provider_auto_import_edits(
            server,
            tsx_path,
            super::super::nav_features_completion_resolve::capture_resolve_surface(
                server, tsx_path,
            )
            .as_ref()
            .map(|(_, snapshot)| &**snapshot),
            &provider_edits,
        );
    assert!(
        resolved.is_err(),
        "a missing IDE context for a REAL carrier must be a structured Err (the \
         non-empty auto-import edits are NOT silently dropped); got {resolved:?}"
    );

    drain.abort();
}

#[test]
fn stale_reconcile_add_gated_when_high_water_advances_in_decide_record_window() {
    // The ATOMICITY of the `reconcile_root_reachability` ADD LOOP: each edge's high-water
    // RE-READ and its slot record must share one critical section (routed through
    // `gate_and_record_root_edge`), so a newer reconcile advancing the per-root high-water
    // BETWEEN this pass's authoritative decide and its add-loop record cannot let the now-
    // stale add re-record an overlay the newer pass already reconciled away and closed.
    //
    // This is the CONCURRENT peer of the sequential
    // `stale_pass_does_not_re_add_an_edge_a_newer_pass_reconciled_away` (in
    // `background_drain_decl_closure::overlay_epoch_tests`): that test advances the
    // high-water BEFORE the stale pass runs, so the stale pass is gated at its DECIDE and
    // never exercises the decide→record WINDOW. Here the stale pass DECIDES it is
    // authoritative (high-water 5 ≤ its gen 5), then BLOCKS at the seam while a newer pass
    // (gen 10) advances the high-water to 10 and drains the slot, then resumes its add loop
    // — the exact reconcile-add-loop TOCTOU the single-edge `open_overlay` interleave test
    // (`stale_open_gated_when_high_water_advances_between_its_gate_and_record`) does not
    // reach.
    //
    // RED-before (the add loop records via `record_root`, NOT the per-edge gate): the stale
    // pass re-records R → OVERLAY at gen 5 after the newer pass closed it — resurrection.
    // GREEN-after (per-edge `gate_and_record_root_edge`): the re-read of the advanced
    // high-water (10 > 5) refuses the stale add and the overlay stays closed.
    const OVERLAY: &str = "/ws/Shared.d.vue.ts";

    let owner = Arc::new(DeclOverlayOwner::default());
    // OVERLAY is reached by root R, edge established by an EARLIER real open (pass gen 2).
    owner.test_seed_slot_with_pass(OVERLAY, &[("R", 2)], 1);

    // Arm the one-shot reconcile add-loop interleave seam for R.
    let interleave = owner.arm_reconcile_add_interleave_for_test("R");

    // Drive the STALE pass (gen 5) on its own thread: it advances R's high-water to 5,
    // decides it is authoritative for the ADD (5 ≥ 5), then BLOCKS at the seam — between
    // the authoritative decide and its add-loop record.
    let owner_stale = Arc::clone(&owner);
    let stale_pass = std::thread::spawn(move || {
        owner_stale.reconcile_root_reachability("R", &[OVERLAY.to_string()], 5)
    });

    // Wait until the stale pass has reached the seam (decided authoritative, about to
    // record). The seam is one-shot — it removed its arming on arrival, so the newer pass
    // below sails straight through it.
    interleave.wait_reached();

    // In the decide→record window: a NEWER pass (gen 10) authoritatively reconciles R to a
    // closure that NO LONGER reaches OVERLAY — advancing R's high-water to 10 and draining
    // the slot to an empty tombstone (the "concurrent reconcile advances + closes in the
    // window").
    let drained = owner.reconcile_root_reachability("R", &[], 10);
    assert_eq!(drained.len(), 1, "the newer pass (gen 10) drained OVERLAY");
    assert_eq!(drained[0].decl_path, OVERLAY);
    assert!(
        owner
            .test_slot_roots(OVERLAY)
            .unwrap_or_default()
            .is_empty(),
        "the newer pass left OVERLAY's reaching set empty"
    );
    assert_eq!(
        owner.test_root_authoritative_epoch("R"),
        10,
        "the newer pass advanced R's authoritative high-water to 10"
    );

    // Release the stale pass into its add-loop record.
    interleave.signal_proceed();
    let stale = stale_pass.join().expect("stale reconcile pass");

    // THE DISCRIMINATOR: the stale pass's add loop must NOT resurrect OVERLAY. The per-edge
    // gate re-reads the high-water (now 10 > 5) and refuses the record, so the slot stays
    // an empty tombstone and the overlay stays closed.
    assert!(
        stale.is_empty(),
        "a stale reconcile decides no closes — got {stale:?}"
    );
    assert!(
        owner
            .test_slot_roots(OVERLAY)
            .unwrap_or_default()
            .is_empty(),
        "the stale pass (gen 5) must NOT re-record R into OVERLAY after a newer pass \
         (gen 10) reconciled it away in the decide→record window — roots={:?}",
        owner.test_slot_roots(OVERLAY)
    );

    // POSITIVE control: a genuinely-CURRENT reconcile (gen 11, at/over the high-water 10)
    // DOES record the edge — the per-edge gate admits authoritative adds, it is not a
    // blanket skip.
    let current = owner.reconcile_root_reachability("R", &[OVERLAY.to_string()], 11);
    assert!(current.is_empty(), "an add does not drain the slot");
    assert!(
        owner
            .test_slot_roots(OVERLAY)
            .unwrap_or_default()
            .contains("R"),
        "a current pass (gen 11, at/over the high-water) re-records R — the per-edge gate \
         admits authoritative adds; roots={:?}",
        owner.test_slot_roots(OVERLAY)
    );
}

/// D3 (atomic / linearizable remove-if-empty) + D2 (over-close root cause). The
/// refcount's remove-if-empty MUST decide emptiness and remove the slot under a
/// SINGLE held shard lock, so a concurrent insert of ANOTHER still-open root's
/// edge into the SAME shared overlay slot is never clobbered (the D3 lost
/// update). D2 is the consequence: if root S's edge is lost when root R is
/// released, S's shared overlay is wrongly closed and S's bare carrier import
/// strands on TS2307 — so the D2 ROOT CAUSE is exactly "S's edge survives R's
/// release", asserted here at the refcount level.
///
/// DISCRIMINATING via a stress loop: two tasks race per iteration — one
/// RELEASES R from a shared slot (`DeclOverlayOwner::release_root`), the other
/// RE-RECORDS S into the SAME slot (`DeclOverlayOwner::reconcile_root_reachability`)
/// starting from a slot reached by R ALONE (`{R}`), exactly the setup where R
/// closes while S is FIRST recording its edge into the same overlay.
///
/// With the atomic remove-if-empty, S's edge ALWAYS survives: either the
/// reconcile lands first (`{R,S}` → release removes R → `{S}`, non-empty, kept),
/// or the release lands first (`{R}` → atomically removed) and the reconcile
/// re-creates `{S}`. The release's emptiness-check-and-remove is ONE held-lock
/// critical section, so it can never delete a slot that the reconcile
/// concurrently populated. The non-atomic pre-fix pattern (snapshot keys →
/// get_mut/check-empty with the guard DROPPED → SEPARATE unconditional `remove`)
/// loses S when: release's get_mut sees `{R}`, removes R → empty → decides
/// drained, then — in the gap before its `remove` — the reconcile inserts S
/// (`{S}`), and release's unconditional `remove` then deletes the now-`{S}` slot.
///
/// DISCRIMINATING via a high-contention stress loop on REAL OS threads aligned by
/// a `Barrier` (so both enter their critical section together), repeated many
/// times. RED-before: the pre-fix non-atomic `remove` loses S in ≥1 race (the
/// test fails reporting the loss count). GREEN-after: zero losses across every
/// iteration. This is a STRESS discriminator (not a single deterministic
/// interleave — the production functions expose no yield point between the
/// emptiness decision and the remove to pin a deterministic interleave), so it is
/// run with enough contention/iterations to make the pre-fix loss reliably
/// observable.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn decl_refcount_remove_if_empty_never_clobbers_concurrent_record() {
    use std::sync::Barrier;

    let shared_overlay = "/ws/Shared.d.vue.ts".to_string();
    let r_root = "/ws/R.vue".to_string();
    let s_root = "/ws/S.vue".to_string();

    let iterations = 300_000usize;
    let s_edge_lost = Arc::new(AtomicUsize::new(0));

    // Real OS threads + a 3-phase barrier per iteration: (1) align both critical
    // sections, (2) both done → measure, (3) measurement done → reset for next
    // iteration. The reset + measure both run on the release thread, bracketed by
    // barriers, so the record thread cannot run during reset/measure and the reset
    // cannot run during measure — the only concurrency is release || record.
    std::thread::scope(|scope| {
        let owner: Arc<DeclOverlayOwner> = Arc::new(DeclOverlayOwner::default());
        let barrier = Arc::new(Barrier::new(2));
        let lost = Arc::clone(&s_edge_lost);

        let owner_a = Arc::clone(&owner);
        let barrier_a = Arc::clone(&barrier);
        let shared_a = shared_overlay.clone();
        let r_a = r_root.clone();
        let s_a = s_root.clone();
        let release_thread = scope.spawn(move || {
            for _ in 0..iterations {
                // Reset the slot to {R} ALONE (the D2 setup).
                owner_a.test_replace_slot(&shared_a, &[r_a.as_str()], 1);
                // Phase (1): aligned start of the concurrent section.
                barrier_a.wait();
                // R closes: drop R from every overlay it reaches.
                owner_a.release_root(&r_a);
                // Phase (2): both critical sections complete — now MEASURE that S
                // still reaches the overlay (R's release must not have clobbered
                // S's concurrent first-time record).
                barrier_a.wait();
                let s_present = owner_a
                    .test_slot_roots(&shared_a)
                    .map(|set| set.contains(&s_a))
                    .unwrap_or(false);
                if !s_present {
                    lost.fetch_add(1, Ordering::Relaxed);
                }
                // Phase (3): measured — safe to reset the slot next iteration.
                barrier_a.wait();
            }
        });

        let owner_b = Arc::clone(&owner);
        let barrier_b = Arc::clone(&barrier);
        let shared_b = shared_overlay.clone();
        let s_b = s_root.clone();
        let record_thread = scope.spawn(move || {
            for _ in 0..iterations {
                // Phase (1): aligned start.
                barrier_b.wait();
                // S records (FIRST time) that it reaches the shared overlay,
                // concurrently with R's release.
                owner_b.reconcile_root_reachability(&s_b, std::slice::from_ref(&shared_b), 1);
                // Phase (2): both complete — the release thread measures.
                barrier_b.wait();
                // Phase (3): next iteration.
                barrier_b.wait();
            }
        });

        release_thread.join().expect("release thread");
        record_thread.join().expect("record thread");
    });

    let lost = s_edge_lost.load(Ordering::Relaxed);
    assert_eq!(
        lost, 0,
        "the atomic remove-if-empty must NEVER clobber a concurrent re-record of a \
         still-open root's edge (D3 lost update → D2 over-close → TS2307); S's edge \
         to the shared overlay vanished in {lost}/{iterations} races"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn closure_reconciles_dropped_import_releases_overlay() {
    // P1 #3 (reconcile): the refcount is NOT append-only. When a root STOPS
    // importing a carrier, the next closure pass must REMOVE that root from the
    // dropped dependency's reachability set — and CLOSE the overlay if its set
    // drains empty. The invariant: a root's recorded reachability edges EQUAL its
    // CURRENT declaration closure each pass (add new, drop dropped), never an
    // ever-growing union.
    //
    // RED-before: the closure only ever INSERTED; removal happened ONLY on
    // did_close. So `B.d.vue.ts` stayed attributed to A (and stayed open) after A
    // stopped importing B.
    let temp_base_guard =
        tempfile::TempDir::with_prefix("verter_test_decl_closure_reconcile").expect("temp dir");
    let temp_base = temp_base_guard.path().to_path_buf();
    let workspace = temp_base.join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("create dirs");
    std::fs::write(
        workspace.join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": "." } }"#,
    )
    .expect("write tsconfig");

    std::fs::write(
        workspace.join("src/B.vue"),
        "<script setup lang=\"ts\">\ndefineProps<{ b: string }>()\n</script>\n<template><div/></template>",
    )
    .expect("write B.vue");
    // A initially imports B.
    let a_with_import = "<script setup lang=\"ts\">\nimport B from './B.vue'\ndefineProps<{ a: string }>()\n</script>\n<template><B/></template>";
    std::fs::write(workspace.join("src/A.vue"), a_with_import).expect("write A.vue");

    let workspace_id_raw = std::fs::canonicalize(&workspace)
        .expect("canonical workspace")
        .to_string_lossy()
        .replace('\\', "/");
    let workspace_id = workspace_id_raw
        .strip_prefix("//?/")
        .unwrap_or(&workspace_id_raw)
        .to_string();
    let a_id = format!("{workspace_id}/src/A.vue");

    let vfs_workspace: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_workspace));
    let documents = DocumentRegistry::new(Arc::clone(&host));
    let uri = crate::uri::path_to_file_uri(&a_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: a_with_import.to_string(),
    });

    let workspace_uri = crate::uri::path_to_file_uri_string(&workspace_id);
    let vite_opts = verter_workspace::ViteConfigOptions::default();
    let registry_ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let build_result = crate::config::ProjectRegistry::from_workspace_roots(
        &registry_ws,
        &[workspace_uri],
        &vite_opts,
    );
    host.configure_projects(
        build_result
            .registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );
    let vfs_workspace = make_test_vfs_workspace_from_registry(&build_result.registry);

    let provider = Arc::new(MockTypeProvider::new());
    let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
    let provider_sync_states = DashMap::new();
    let decl_overlay_owner = DeclOverlayOwner::default();

    // Pass 1: A imports B → B.d opened + recorded as reached from A.
    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        true,
        None,
        &decl_overlay_owner,
        1,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;

    let b_reached_by_a = || {
        decl_overlay_owner
            .test_slots_snapshot()
            .iter()
            .any(|(key, roots)| {
                key.contains("B.d.vue.ts") && roots.iter().any(|root| root.ends_with("/A.vue"))
            })
    };
    assert!(
        b_reached_by_a(),
        "pass 1: B.d.vue.ts is recorded as reached from A, slots={:?}",
        decl_overlay_owner.test_slots_snapshot()
    );

    // Edit A to DROP the `import B` (now imports nothing).
    let a_no_import = "<script setup lang=\"ts\">\ndefineProps<{ a: string }>()\n</script>\n<template><div/></template>";
    let _ = documents.did_change(&uri, 2, a_no_import);
    provider.clear_calls();

    // Pass 2: A no longer imports B → reconcile must drop A from B.d's set and,
    // since A was the only reaching root, CLOSE B.d.vue.ts.
    resync_aliased_imports_for_open_files(
        &documents,
        Some(&sync),
        &vfs_workspace,
        &provider_sync_states,
        true,
        None,
        &decl_overlay_owner,
        1,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
        &dashmap::DashSet::new(),
    )
    .await;

    assert!(
        !b_reached_by_a(),
        "pass 2: A is dropped from B.d.vue.ts's reachability after A stops importing B, slots={:?}",
        decl_overlay_owner.test_slots_snapshot()
    );

    let calls = provider.file_sync_calls();
    let closed_b_decl = calls
        .iter()
        .any(|call| matches!(call, MockCall::CloseFile { path } if path.contains("B.d.vue.ts")));
    assert!(
        closed_b_decl,
        "pass 2: B.d.vue.ts is CLOSED once its last reaching root (A) drops the import, calls={calls:?}"
    );

    let _ = std::fs::remove_dir_all(&temp_base);
}

#[test]
fn carrier_dependency_ids_resolves_carriers_and_filters_non_carriers() {
    // P2 #4: `carrier_dependency_ids` resolves each script import through the
    // engine's workspace resolver (analysis-time `resolved_canonical_id`, with the
    // `resolve_import_specifier_standalone` → `host.resolve_for_persistent_state`
    // fallback rail) and returns ONLY the carrier→carrier edges — a non-carrier
    // (`.ts`) dependency is dropped (handled by the other passes), per the
    // documented contract. (The fallback rail itself is a timing safety net for the
    // registry-bootstrap window where analysis ran before the project registry was
    // configured; its discriminating coverage is the aliased-import resync flow.
    // The analysis-time id and the workspace resolver are architecturally coupled —
    // both bottom out in the same resolver — so they cannot be split in a single-
    // state unit test. This test pins the contract the closure depends on: resolve
    // + carrier-filter.)
    //
    // Discriminates against (a) dropping resolution entirely (empty result), and
    // (b) failing to carrier-filter (a `.ts` dep leaking into the declaration
    // closure, which would open a `.d.ts` for a non-carrier).
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let host = VerterHost::new(HostConfig::default(), ws);
    host.configure_projects(vec![verter_workspace::ide_project_config(
        "/src".to_string(),
        "/src".to_string(),
        Some("/src/tsconfig.json".to_string()),
    )]);
    host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: "/src/A.vue".to_string(),
        source: Arc::from(
            "<script setup lang=\"ts\">\nimport B from './B.vue'\nimport { util } from './util'\n</script>\n<template><B>{{ util }}</B></template>",
        ),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    })
    .expect("upsert A.vue");
    host.set_exact_resolutions(
        "/src/A.vue",
        vec![
            verter_workspace::ExactResolution {
                specifier: "./B.vue".to_string(),
                phase: verter_session_query::resolution::ResolvePhase::CodegenBlocker,
                kind: verter_session_query::resolution::ResolveRequestKind::EsmImport,
                resolved_canonical_id: Some("/src/B.vue".to_string()),
                possible_canonical_ids: vec!["/src/B.vue".to_string()],
            },
            verter_workspace::ExactResolution {
                specifier: "./util".to_string(),
                phase: verter_session_query::resolution::ResolvePhase::CodegenBlocker,
                kind: verter_session_query::resolution::ResolveRequestKind::EsmImport,
                resolved_canonical_id: Some("/src/util.ts".to_string()),
                possible_canonical_ids: vec!["/src/util.ts".to_string()],
            },
        ],
    );

    let deps = carrier_dependency_ids(&host, "/src/A.vue");
    let deps = deps.expect("exact fixture resolutions should be admitted");

    // POSITIVE: the carrier dependency `./B.vue` is resolved and returned.
    assert!(
        deps.iter().any(|d| d == "/src/B.vue"),
        "carrier_dependency_ids resolves and returns the carrier dependency ./B.vue, got: {deps:?}"
    );
    // NEGATIVE (discriminating): the non-carrier `./util` (a `.ts`) is FILTERED —
    // the closure follows only carrier→carrier edges, so a `.ts` dep must never be
    // opened as a declaration overlay.
    assert!(
        !deps.iter().any(|d| d == "/src/util.ts"),
        "carrier_dependency_ids must FILTER the non-carrier ./util.ts dependency, got: {deps:?}"
    );
}

/// A→B atomic swap through the PRODUCTION interactive entry
/// `ensure_current_file_synced`: a source that moves from owner A to owner B must
/// leave NOTHING advertised under A in the ledger-backed `getExternalFiles` (the
/// source-indexed single-entry swap). DISCRIMINATING: a publish-then-prune impl
/// that added B without removing the A entry would leave `advertised_under(A)`
/// non-empty.
#[tokio::test(flavor = "multi_thread")]
async fn owner_change_a_to_b_through_production_leaves_nothing_under_a() {
    use crate::external_ts::{CanonicalSource, ProjectUri};
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();

    // Two configs at the SAME root, each owning `{root}/src/App.vue` via `**/*`.
    let tsconfig_a = "/workspace/tsconfig.json";
    let tsconfig_b = "/workspace/tsconfig.app.json";
    let canonical_id = "/workspace/src/App.vue";

    install_test_resolver_for_root(server, "/workspace", Some(tsconfig_a));
    let uri = open_test_vue(server, canonical_id, MEMBERSHIP_TEST_VUE);
    server.ensure_current_file_synced(&uri).await;

    assert_eq!(
        server
            .membership_ledger()
            .expect("tsserver has a ledger")
            .advertised_under(&ProjectUri::from(tsconfig_a)),
        vec![CanonicalSource::from(canonical_id)],
        "after the A publish the source is advertised under A"
    );

    // The owner CHANGES to B (same root, new tsconfig).
    install_test_resolver_for_root(server, "/workspace", Some(tsconfig_b));
    server.ensure_current_file_synced(&uri).await;

    let ledger = server.membership_ledger().expect("tsserver has a ledger");
    assert!(
        ledger
            .advertised_under(&ProjectUri::from(tsconfig_a))
            .is_empty(),
        "the A→B swap MUST leave NOTHING advertised under the old project A, got {:?}",
        ledger.advertised_under(&ProjectUri::from(tsconfig_a))
    );
    assert_eq!(
        ledger.advertised_under(&ProjectUri::from(tsconfig_b)),
        vec![CanonicalSource::from(canonical_id)],
        "the source must be advertised under the new project B after the swap"
    );
}

/// Gap (c) — `sync_imported_carrier_api_lightweight` owner loss must retract the
/// ledger membership of an imported child carrier whose owner is gone.
/// DISCRIMINATING: pre-change the imported-child owner-loss branch corrected the
/// binding but left the STORE/ledger membership advertised.
#[tokio::test(flavor = "multi_thread")]
async fn imported_child_owner_loss_retracts_ledger_membership() {
    use crate::external_ts::CanonicalSource;
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    let tsconfig = "/workspace/tsconfig.json";
    let canonical_id = "/workspace/src/Child.vue";
    install_test_resolver_for_root(server, "/workspace", Some(tsconfig));
    let uri = open_test_vue(server, canonical_id, MEMBERSHIP_TEST_VUE);

    // Publish under the resolved owner.
    server.ensure_current_file_synced(&uri).await;
    assert!(
        server
            .membership_ledger()
            .expect("ledger")
            .is_advertised(&CanonicalSource::from(canonical_id)),
        "precondition: the imported child carrier is advertised after the owned publish"
    );

    // Owner loss, then drive the imported-child lightweight sync.
    install_test_resolver_for_root(server, "/other", Some("/other/tsconfig.json"));
    server
        .sync_imported_carrier_api_lightweight(canonical_id)
        .await;

    assert!(
        !server
            .membership_ledger()
            .expect("ledger")
            .is_advertised(&CanonicalSource::from(canonical_id)),
        "imported-child owner-loss MUST retract the ledger-backed getExternalFiles membership"
    );
}

/// A default import of a `.svelte` child component resolves to the
/// child carrier through the carrier-generic `is_default_export_component_carrier`
/// gate — exactly like a `.vue` child. DISCRIMINATING: the pre-change
/// `ends_with(".vue")` gate would skip the `.svelte` resolved target and the
/// component would not resolve.
#[tokio::test]
async fn component_resolve_targets_svelte_carrier() {
    let mock = Arc::new(MockTypeProvider::new());
    let service = make_hover_test_service(mock.clone());
    let server = service.inner();
    install_test_resolver(server);

    open_test_svelte(
        server,
        "/workspace/src/Child.svelte",
        "<script>let x = 1;</script>",
    );
    let parent_uri = open_test_svelte(
        server,
        "/workspace/src/App.svelte",
        "<script>import Child from './Child.svelte';</script>\n<Child />\n",
    );
    let parent_analysis = server
        .documents
        .get_analysis(&parent_uri)
        .expect("parent analysis");

    let resolved = server.resolve_component_document_for_import_binding(
        &parent_uri,
        &parent_analysis,
        "./Child.svelte",
        "Child",
    );
    let resolved =
        resolved.expect("a default import of a .svelte child must resolve to the carrier");
    assert!(
        resolved.uri.as_str().ends_with("Child.svelte"),
        "the resolved component-target must be the .svelte carrier, got {}",
        resolved.uri.as_str()
    );
}

/// F1 (type split — `active_non_decl_paths` EXCLUDES the declaration overlay). The
/// generic stale-path closers consume the close-target set this method returns; a
/// declaration overlay (`Decl`) must never appear in it, because a `Decl` overlay's
/// lifecycle is owned exclusively by `DeclOverlayOwner` and must never be closed by
/// a generic stale-path close (closing it while an open carrier root still reaches
/// it strands that root on TS2307).
///
/// Discriminates directly: a state carrying ALL FOUR provider paths yields exactly
/// the three non-decl kinds from `active_non_decl_paths()` (Decl absent) while
/// `active_paths()` still yields all four — so the generic closers, which take the
/// `NonDeclProviderPathKind` slice, are structurally unable to receive the decl
/// path. RED-before (the method does not exist on the pre-fix tree — a compile
/// error); GREEN-after.
#[test]
fn active_non_decl_paths_excludes_the_declaration_overlay() {
    use crate::provider_sync::{NonDeclProviderPathKind, ProviderPathKind, ProviderSyncState};

    let state = ProviderSyncState {
        owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
            "/ws/tsconfig.json".into(),
        ),
        ide_path: Some("/ws/App.vue.tsx".to_string()),
        api_path: Some("/ws/App.vue.verter.ts".to_string()),
        decl_path: Some("/ws/App.d.vue.ts".to_string()),
        shadow_path: Some("/ws/App.shadow".to_string()),
        ide_background_loaded: true,
        api_background_loaded: true,
        decl_background_loaded: true,
        shadow_background_loaded: true,
        committed_ide_surface: None,
        commit_stamp: None,
        api_delivered_hash: None,
        api_observed_hash: None,
        shadow_delivered_source_hash: None,
    };

    // The full active set still includes the declaration overlay.
    let all: Vec<ProviderPathKind> = state.active_paths().into_iter().map(|(k, _)| k).collect();
    assert!(
        all.contains(&ProviderPathKind::Decl),
        "active_paths() still includes the Decl overlay, got {all:?}"
    );

    // The generic-closer input EXCLUDES the declaration overlay entirely.
    let non_decl = state.active_non_decl_paths();
    let kinds: Vec<NonDeclProviderPathKind> = non_decl.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        kinds,
        vec![
            NonDeclProviderPathKind::Ide,
            NonDeclProviderPathKind::Api,
            NonDeclProviderPathKind::Shadow,
        ],
        "active_non_decl_paths() returns exactly Ide/Api/Shadow (no Decl), got {kinds:?}"
    );
    assert!(
        !non_decl.iter().any(|(_, path)| path == "/ws/App.d.vue.ts"),
        "the declaration overlay path must NEVER appear in the generic-closer input, got {non_decl:?}"
    );
}

#[tokio::test]
async fn contract_unknown_custom_directive_name_is_silent() {
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", D6_PARENT_SOURCE)]).await;
    let server = service.inner();
    let uri = workspace_uri(&workspace_id, "src/App.vue");
    let position = find_document_position(server, &uri, "v-nope", 2);

    let hover = server
        .hover(hover_params(&uri, position))
        .await
        .expect("hover request should succeed");
    assert!(
        hover.is_none(),
        "unknown custom directive must produce no hover, got: {hover:?}"
    );
    let response = server
        .goto_definition(goto_definition_params(&uri, position))
        .await
        .expect("goto definition should succeed");
    assert!(
        response.is_none(),
        "unknown custom directive must produce no definition, got: {response:?}"
    );
    drain_handle.abort();
    drop(service);
}

/// The committed VS Code E2E fixture stays semantically tied to the boundary
/// path: the EXACT fixture bytes produce the three unused-declaration hints
/// (a drift in the fixture or the pipeline breaks the E2E and this test
/// together, pointing at the same cause).
#[test]
fn e2e_fixture_unused_declarations_matches_boundary_semantics() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../packages/vue-vscode/e2e/fixtures/vue-parity/src/diagnostics/UnusedDeclarations.vue",
    );
    let source = std::fs::read_to_string(&fixture).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("UnusedDeclarations.vue");
    std::fs::write(&file, &source).unwrap();

    let host = crate::test_utils::make_filesystem_test_host(dir.path());
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri =
        crate::uri::path_to_file_uri(&file.to_string_lossy().replace('\\', "/")).expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let cached_verter_diags = Arc::new(DashMap::new());
    let diags =
        crate::server::document_diagnostics_for_test(&documents, &uri, &cached_verter_diags, None);
    let count = |code: &str| {
        diags
            .iter()
            .filter(|d| matches!(d.code.as_ref(), Some(NumberOrString::String(c)) if c == code))
            .count()
    };
    assert_eq!(count("verter/no-unused-props"), 1, "props");
    assert_eq!(count("verter/no-unused-emit-declarations"), 1, "emits");
    assert_eq!(count("verter/no-unused-slots"), 1, "slots");
}

/// The receipt's INVALIDATION direction: editing an IMPORTED file's CONTENTS,
/// with its import set unchanged, must make the publication enqueued by the
/// next definition re-push that carrier WITH THE NEW BYTES.
///
/// This is the half a zero-resync assertion cannot cover. The receipt key rides
/// the workspace `content_generation`, which any content edit bumps — including
/// an edit to a dependency the requesting document merely imports. Without
/// that, a warm capture would serve the engine stale companion bytes
/// indefinitely.
#[tokio::test]
async fn editing_an_imported_carrier_re_pushes_it_with_the_new_bytes() {
    let child_source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [payload: string] }>()\n</script>\n";
    let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleCustom(payload: string) {}\n</script>\n<template>\n  <MyComp @custom=\"handleCustom\" />\n</template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server_with_kind(
            &[
                ("src/MyComp.vue", "vue", child_source),
                ("src/App.vue", "vue", parent_source),
            ],
            crate::TypeProviderKind::Tsgo,
        )
        .await;

    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);
    let child_id = format!("{workspace_id}/src/MyComp.vue");

    // Mint the receipt: the cold definition enqueues the background
    // publication; after it settles the steady state pushes nothing.
    let _ = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let canonical = server
            .documents
            .get(&app_uri)
            .expect("parent stays open")
            .canonical_id
            .clone();
        let key = server
            .import_sync_freshness_key()
            .expect("host generations");
        server.import_sync.wait_fresh_at(&canonical, key).await;
    })
    .await
    .expect("the cold definition must enqueue a publication that mints DependencyReady");

    let child_pushes = |calls: Vec<MockCall>| -> Vec<String> {
        calls
            .into_iter()
            .filter_map(|call| match call {
                MockCall::OpenFile { path, content }
                | MockCall::UpdateFile { path, content }
                | MockCall::LoadFile { path, content } => {
                    path.starts_with(&child_id).then_some(content)
                }
                _ => None,
            })
            .collect()
    };
    let warm_pushes = child_pushes(provider.calls());
    assert!(
        !warm_pushes.is_empty(),
        "the settled publication must actually have pushed the imported child's \
         companions, else this test proves nothing"
    );
    let already_pushed = warm_pushes.len();

    // Edit the IMPORTED child's CONTENTS ONLY. The parent's import set is
    // untouched — only the bytes behind the import change.
    let edited_child = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [payload: number] }>()\n</script>\n";
    server
        .documents
        .host()
        .upsert(UpsertRequest {
            canonical_id: Some(child_id.clone()),
            input_id: child_id.clone(),
            source: edited_child.into(),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert the edited imported child");

    // The content-generation bump invalidated the receipt by key: the next
    // definition misses, enqueues a fresh publication, and THAT re-pushes the
    // child with the new bytes.
    let _ = server
        .goto_definition(goto_definition_params(&app_uri, position))
        .await;

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        provider
            .wait_until_calls(|calls| child_pushes(calls.to_vec()).len() > already_pushed)
            .await;
    })
    .await
    .expect(
        "an edited imported carrier must be re-pushed by the publication the next \
         definition enqueues — the receipt must not warm-skip a content change \
         behind the import",
    );
    let after_edit: Vec<String> = child_pushes(provider.calls())
        .into_iter()
        .skip(already_pushed)
        .collect();
    assert!(
        after_edit.iter().any(|content| content.contains("number")),
        "the re-push must carry the NEW bytes, not the receipted ones; got: {after_edit:?}"
    );

    drain_handle.abort();
    drop(service);
}

/// Production must never turn a valid cold-project response into an empty one
/// merely because it exceeded a guessed latency budget.
#[test]
fn production_feature_request_budgets_are_disabled() {
    let budgets = verter_session::LspMethodBudgets::interactive_defaults();
    for (name, budget) in [
        ("hover", budgets.hover),
        ("goto_definition", budgets.goto_definition),
        ("completion", budgets.completion),
        ("references", budgets.references),
        ("code_action", budgets.code_action),
        ("rename", budgets.rename),
        ("diagnostics", budgets.diagnostics),
        ("other", budgets.other),
    ] {
        assert!(
            budget.is_zero(),
            "{name} must be unbounded in production, got {budget:?}"
        );
    }
}

/// The acceptance signal: over a mixed batch of healthy and wedged definitions,
/// the shortened budget answers every healthy request (the answered count does
/// not drop) and fails every wedged one closed at its budget rather than at 15s
/// — the dead-tail wait cut ~6x with no loss of answered work.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shortened_budget_cuts_the_dead_tail_without_dropping_answered_requests() {
    const HEALTHY: usize = 12;
    let budget = std::time::Duration::from_millis(2500);

    // Healthy cohort: a provider that answers definition immediately.
    let healthy_provider = Arc::new(MockTypeProvider::new());
    let (healthy_service, _h) = wedged_provider_server(Arc::clone(&healthy_provider), |config| {
        config.lsp_method_timeouts.request_deadlines.goto_definition = budget;
    });
    let healthy_server = healthy_service.inner();
    let healthy_uri = open_test_vue(
        healthy_server,
        "/workspace/src/App.vue",
        DEADLINE_TEST_SOURCE,
    );
    let healthy_pos = find_document_position(healthy_server, &healthy_uri, "{{ count", 3);

    let mut answered = 0usize;
    for _ in 0..HEALTHY {
        let r = super::super::nav_features_audit::handle_goto_definition_with_audit(
            healthy_server,
            goto_definition_params(&healthy_uri, healthy_pos),
        )
        .await;
        if r.is_ok() {
            answered += 1;
        }
    }
    assert_eq!(
        answered, HEALTHY,
        "every healthy definition must still be answered — the answered count must not drop"
    );

    // Wedged cohort: a provider whose definition never returns. Each must fail
    // closed at the budget, far below the old 15s.
    let wedged_provider = Arc::new(MockTypeProvider::new());
    wedged_provider.hang_definition();
    let (wedged_service, _w) = wedged_provider_server(Arc::clone(&wedged_provider), |config| {
        config.lsp_method_timeouts.request_deadlines.goto_definition = budget;
    });
    let wedged_server = wedged_service.inner();
    let wedged_uri = open_test_vue(
        wedged_server,
        "/workspace/src/App.vue",
        DEADLINE_TEST_SOURCE,
    );
    let wedged_pos = find_document_position(wedged_server, &wedged_uri, "{{ count", 3);

    // Production-shaped readiness (surface + receipt), so the request reaches
    // the wedged provider hop instead of answering natively without it.
    wedged_server.ensure_current_file_synced(&wedged_uri).await;
    wedged_server
        .publish_import_dependencies_settled(&wedged_uri)
        .await;

    let wedged = super::super::nav_features_audit::handle_goto_definition_with_audit(
        wedged_server,
        goto_definition_params(&wedged_uri, wedged_pos),
    )
    .await;

    let err = wedged.expect_err("a wedged definition must fail closed");
    assert_eq!(
        err.code,
        tower_lsp_server::jsonrpc::ErrorCode::RequestCancelled,
        "the wedged request must fail closed as request_cancelled, got {err:?}"
    );
}

/// A burst of `didChange` notifications arriving on the wire must not cost one
/// IDE compile per notification.
///
/// This is the discriminating test for https://github.com/pikax/verter/issues/96.
/// It drives the real `LspService` + serve-loop ingress, stages nothing by
/// hand, and counts COLD COMPILE RUNS (a warm `ensure_compile_artifacts` hit
/// does not move the rail, and a FAILED compile does) rather than provider
/// updates.
///
/// It asserts BOTH halves of the contract, and both are deterministic:
///
///   * **Zero while the burst is in flight.** The test holds a
///     [`ChangeInFlight`] ticket for this canonical id across the whole burst.
///     A document with a change in flight is excluded from the coordinator's
///     deadline computation AND its dispatch set, so the coordinator provably
///     cannot dispatch while that ticket is alive. Any compile counted here
///     therefore came from the notification-handling path — the #96 bug.
///     Without the ticket this measurement is a race: the debounce is measured
///     from handler RECEIPT, so a 24-edit backlog that takes longer than
///     `DEBOUNCE_MS` to commit leaves the window already elapsed the moment the
///     last handler finishes, and the legitimate debounced compile can land
///     before the sample.
///
///   * **Exactly one after quiescence.** Dropping the ticket lets the file go
///     quiet; the coordinator then owes exactly one refresh for the settled
///     revision — the debt the commit path no longer pays. This doubles as the
///     positive control: it proves the rail is live, so the zero above is not a
///     dead counter, and it proves the burst's work was DEFERRED rather than
///     silently skipped.
///
/// One is the whole point of the design: 24 notifications, one compile. A
/// per-notification compile reads 24 at the first assertion.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_burst_of_did_change_notifications_does_not_compile_once_per_notification() {
    const BURST: usize = 24;
    let source = "<script setup lang=\"ts\">\nconst count = 1\n</script>\n\
                  <template><div>{{ count }}</div></template>\n";
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let service = ingress_measurement_server(&host);
    let uri = open_test_vue(service.inner(), "/workspace/src/App.vue", source);
    let canonical_id = crate::documents::uri_to_canonical_id(&uri);
    let profile = service.inner().documents.tsx_profile.read().clone();
    assert!(
        host.get_ide(&canonical_id, &profile).is_some(),
        "precondition: the OPEN must have produced the carrier's IDE TSX, so the \
         burst below starts from an established projection rather than bootstrapping one"
    );
    let coordinator = service.inner().sync_coordinator.clone();

    let (mut client_to_server, serve) = serve_over_duplex_initialized(service).await;

    // Pin the document non-quiescent for the whole burst. This is what makes
    // the in-flight measurement a fact rather than a race.
    let ticket = coordinator.change_received(canonical_id.clone());

    // Baseline taken AFTER the session is initialized, so nothing the handshake
    // does is attributed to the burst.
    let before = cold_compile_runs(&host);
    let (frames, final_source) = did_change_burst_frames(&uri, 2, BURST);
    {
        use tokio::io::AsyncWriteExt;
        client_to_server
            .write_all(&frames)
            .await
            .expect("the burst must reach the server");
        client_to_server.flush().await.expect("flush the burst");
    }

    // Fence on the LAST revision being committed. Waiting longer can only let
    // MORE commit-path compiles land, and the held ticket keeps the debounced
    // one off, so this can never turn a real per-notification compile into a
    // pass.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while host.get_source(&canonical_id).as_deref() != Some(final_source.as_str()) {
        assert!(
            std::time::Instant::now() < deadline,
            "precondition: every queued notification must be committed; the document \
             is still at {:?}",
            host.get_source(&canonical_id)
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let in_flight = cold_compile_runs(&host) - before;

    assert_eq!(
        in_flight, 0,
        "{BURST} didChange notifications started {in_flight} cold compile run(s) on the \
         notification-handling path. `tower-lsp-server` polls handler futures inline \
         and each one runs from entry through commit without pending, so a compile in \
         the commit is a serialized queue as long as the user's typing burst — the ~9s \
         of issue #96. The commit owes the document's TEXT; the TSX is owed by whoever \
         demands it"
    );

    // Release the ticket: the file may now go quiet, and the coordinator owes
    // exactly one compile for the settled revision.
    drop(ticket);
    let settled = await_settled_cold_compiles(&host, before).await;
    assert_eq!(
        settled, 1,
        "after the burst went quiet the debounced coordinator must compile the \
         settled revision EXACTLY once ({settled} observed). Zero means the \
         deferred work is never done — Verter's own diagnostics would go empty \
         and stay empty, which is the regression this pairs with. More than one \
         means the refresh is no longer coalesced onto the quiet window"
    );
    serve.abort();
}

/// The coordinator releases the lane after its IDE leg and an interactive
/// request takes it before the API leg asks again. Yielding to that request is
/// contention, not a failed attempt: the transaction reports a lane yield, which
/// the serial loop requeues without spending the document's retry budget.
#[tokio::test(flavor = "multi_thread")]
async fn an_api_leg_yielding_to_an_interactive_request_is_a_lane_yield_not_a_retry() {
    let (service, _provider, uri) = lane_interleaving_fixture("ApiLegYields").await;
    let server = service.inner();
    let id = crate::documents::uri_to_canonical_id(&uri);
    edit_interleaving_document(server, &uri, 2, "world");
    let deps = lane_interleaving_deps(server);
    let (arrived, release) = crate::sync_coordinator::test_hooks::block_before_delivery(&id);
    let interactive = async {
        arrived.notified().await;
        let held = match server.documents.try_delivery_lane(&id) {
            crate::document_sync_lane::DeliveryLane::Acquired(guard) => guard,
            other => panic!("the IDE leg released the lane, got {other:?}"),
        };
        release.notify_one();
        held
    };
    let (outcome, held) = tokio::join!(
        crate::sync_coordinator::synchronize_document_outcome_for_test(&deps, &id, uri.as_str()),
        interactive
    );
    drop(held);
    assert_eq!(
        outcome,
        crate::sync_coordinator::SyncFileOutcome::LaneBusy,
        "an API leg that yields the lane must not read as a failed transaction"
    );
    assert!(
        server.pending_snapshot_provider_sync.contains(&id),
        "the yielded API leg stays owed"
    );
}

/// The imported-carrier writer releases the child's lane between its legs. Its
/// unresolved API leg must then wait its turn behind a lane holder, exactly as
/// the resolved arm does, rather than return a retry its callers discard —
/// leaving the leg undelivered with nothing to redrive it.
#[tokio::test(flavor = "multi_thread")]
async fn an_unresolved_api_leg_waits_its_turn_behind_the_lane_holder() {
    let (service, _provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();
    let id = crate::documents::uri_to_canonical_id(&uri);
    let api_code = server
        .documents
        .host()
        .get_public_api(&id)
        .expect("public API projection")
        .expect("the carrier has a public API")
        .ts_labeled_code()
        .to_string();
    let held = match server.documents.try_delivery_lane(&id) {
        crate::document_sync_lane::DeliveryLane::Acquired(guard) => guard,
        other => panic!("the open document's lane is free, got {other:?}"),
    };
    let leg = server.sync_carrier_api_unresolved(&id, &api_code);
    tokio::pin!(leg);
    assert!(
        futures_util::poll!(leg.as_mut()).is_pending(),
        "the leg waits for the holder instead of giving up"
    );
    drop(held);
    tokio::time::timeout(std::time::Duration::from_secs(10), leg)
        .await
        .expect("the leg runs once the holder releases the lane");
}
