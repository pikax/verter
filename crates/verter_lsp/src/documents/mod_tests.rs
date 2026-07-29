//! Tests for the plain-script self-file projection and the JSX-dialect
//! fallback (`DocumentRegistry::is_jsx` / `is_jsx_for_canonical`).

use super::*;

#[test]
fn did_open_rune_module_builds_self_file_projection_with_prelude_offset() {
    use provider_projection::DocumentProviderProjection;
    use verter_span::{LspPosition, TsPosition};

    let host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig::default(),
    ));
    let registry = DocumentRegistry::new(host);
    let uri: Uri = "file:///x/store.svelte.ts".parse().expect("uri");
    let _ = registry.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "typescript".into(),
        version: 1,
        text: "export const s = $state(0);\n".into(),
    });

    let projection = registry.get_projection(&uri).expect("rune projection");
    let DocumentProviderProjection::SelfFile { mapper } = projection else {
        panic!("a rune module must use a self-file projection")
    };
    let prelude = mapper.prelude_line_count();
    assert!(prelude > 0);
    let mapper = registry.get_position_mapper(&uri).expect("unified mapper");
    assert_eq!(
        mapper
            .carrier_to_tsx(LspPosition::new(0, 13))
            .expect("source maps")
            .pos,
        TsPosition::new(prelude, 13)
    );
    assert_eq!(
        mapper
            .tsx_to_carrier(TsPosition::new(prelude, 13))
            .expect("provider maps")
            .pos,
        LspPosition::new(0, 13)
    );
    assert!(mapper.tsx_to_carrier(TsPosition::new(0, 0)).is_none());
}

mod source_identity_fence {
    use super::*;

    fn registry_with_host() -> DocumentRegistry {
        DocumentRegistry::new(Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig::default(),
        )))
    }

    const REVISION_A: &str =
        "<script setup lang=\"ts\">\nconst msg = 'a'\n</script>\n<template><div>{{ msg }}</div></template>\n";
    const REVISION_B: &str =
        "<script setup lang=\"ts\">\nconst msg = 'b'\nconst extra = 2\n</script>\n<template><div>{{ msg }}{{ extra }}</div></template>\n";

    fn projectionless_registry() -> (DocumentRegistry, Uri) {
        let registry = registry_with_host();
        let uri: Uri = "file:///x/App.vue".parse().expect("uri");
        let _ = registry.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".into(),
            version: 1,
            text: REVISION_A.into(),
        });
        assert!(
            registry.get_projection(&uri).is_some(),
            "precondition: revision A compiles"
        );
        registry.clear_projection_for_test(&uri);
        (registry, uri)
    }

    /// A source move while the blocking compile runs must discard its output.
    #[test]
    fn a_compile_whose_source_moved_installs_no_mapper_and_returns_nothing() {
        let (registry, uri) = projectionless_registry();
        registry.set_after_compile_hook(Box::new(|registry, uri| {
            let _ = registry.did_change(uri, 2, REVISION_B);
        }));

        let response = registry.recompile_and_refresh_mapper(&uri);

        assert!(
            registry
                .get(&uri)
                .is_some_and(|doc| doc.source.as_ref() == REVISION_B),
            "the interleaved edit must commit revision B"
        );
        assert!(
            response.is_none(),
            "revision A's retained response must be discarded"
        );
        assert!(
            registry.get_projection(&uri).is_none(),
            "revision A's mapper must not be installed over revision B"
        );
    }

    /// The comparison must also hold at the projection write itself. This
    /// pauses after the early check and commits B in the check→`get_mut` window.
    #[test]
    fn a_source_change_after_the_check_cannot_install_the_retained_mapper() {
        let (registry, uri) = projectionless_registry();
        registry.set_before_projection_install_hook(Box::new(|registry, uri| {
            let _ = registry.did_change(uri, 2, REVISION_B);
        }));

        let response = registry.recompile_and_refresh_mapper(&uri);

        assert!(
            registry
                .get(&uri)
                .is_some_and(|doc| doc.source.as_ref() == REVISION_B),
            "the post-check edit must commit revision B"
        );
        assert!(
            response.is_none(),
            "revision A's retained response must not escape after revision B \
             commits at the installation write point"
        );
        assert!(
            registry.get_projection(&uri).is_none(),
            "revision A's mapper must not be installed over revision B"
        );
    }

    fn compile_error(code: &str) -> Result<bool, verter_session::HostError> {
        Err(verter_session::HostError::CompileError(
            verter_session::CompileFailure {
                diagnostics: verter_session::DiagnosticsSnapshot {
                    diagnostics: vec![verter_session::HostDiagnostic {
                        severity: verter_session::HostSeverity::Error,
                        code: code.to_string(),
                        message: String::new(),
                        span: None,
                    }],
                    has_errors: true,
                },
                requested_mode: verter_session::CompileCacheMode::Content,
                actual_mode: verter_session::CompileCacheMode::Content,
                downgrade_reason: None,
            },
        ))
    }

    /// Cancellation-shaped macro semantic failures are transient and must not
    /// bind unchanged bytes across requests.
    #[test]
    fn cancellation_codes_never_bind_projection_repair_content() {
        let registry = registry_with_host();
        const CANONICAL: &str = "/x/App.vue";
        const SOURCE: &str = "<script setup lang=\"ts\">defineProps<P>()</script>";

        for code in [
            verter_compiler::diagnostics::X_MISSING_MACRO_SEMANTIC_BUNDLE,
            verter_compiler::diagnostics::X_UNAVAILABLE_MACRO_SEMANTIC_RESULT,
        ] {
            registry.account_carrier_ide_content_verdict(
                CANONICAL,
                SOURCE,
                &compile_error(code),
                false,
                false,
            );
            assert!(
                !registry.carrier_ide_compile_has_content_verdict(CANONICAL, SOURCE),
                "{code} is transient unavailable input and must remain retryable"
            );
        }

        registry.account_carrier_ide_content_verdict(
            CANONICAL,
            SOURCE,
            &compile_error("XInvalidExpression"),
            false,
            false,
        );
        assert!(
            registry.carrier_ide_compile_has_content_verdict(CANONICAL, SOURCE),
            "the seam must still bind a deterministic compiler verdict"
        );
    }
}

#[test]
fn position_mapper_not_overwritten_when_present() {
    let host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig::default(),
    ));
    let registry = DocumentRegistry::new(host);
    let uri: Uri = "file:///home/user/App.vue".parse().expect("uri");
    let _ = registry.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".into(),
        version: 1,
        text: "<template><div>hello</div></template><script setup lang=\"ts\">\nconst x = 1;\n</script>".into(),
    });

    assert!(registry.get_position_mapper(&uri).is_some());
    assert!(registry.get_ide(&uri).is_some());
    assert!(registry.get_position_mapper(&uri).is_some());
}

/// A plain TS-family script is NOT a carrier either: did_open must build a
/// SELF-FILE projection whose zero-prelude mapper is the identity (its
/// provider buffer is the source bytes verbatim). Without the projection,
/// every provider-backed feature (hover/definition/completion/diagnostics)
/// fails closed for plain scripts.
#[test]
fn did_open_plain_script_builds_self_file_projection_with_identity_mapping() {
    use provider_projection::DocumentProviderProjection;
    use verter_span::{LspPosition, TsPosition};

    let host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig::default(),
    ));
    let registry = DocumentRegistry::new(Arc::clone(&host));

    let uri: Uri = "file:///x/plain-control.ts".parse().expect("uri");
    let _ = registry.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const plainControlNumber = 1;\nplainControlNumber.toFixed(0);\n".to_string(),
    });

    let projection = registry
        .get_projection(&uri)
        .expect("a plain .ts script must build a self-file provider projection");
    let mapper = match &projection {
        DocumentProviderProjection::SelfFile { mapper } => mapper.clone(),
        DocumentProviderProjection::CarrierIde { .. } => {
            panic!("a plain .ts script is NOT a carrier — must be a SelfFile projection")
        }
    };
    assert_eq!(
        mapper.prelude_line_count(),
        0,
        "a plain script's provider buffer is verbatim — no prelude offset"
    );

    // Identity mapping in both directions (zero prelude, no rewrites).
    let prov = registry
        .get_position_mapper(&uri)
        .expect("unified mapper")
        .carrier_to_tsx(LspPosition::new(1, 3))
        .expect("source maps to provider");
    assert_eq!(prov.pos, TsPosition::new(1, 3));
    let back = registry
        .get_position_mapper(&uri)
        .expect("unified mapper")
        .tsx_to_carrier(TsPosition::new(1, 3))
        .expect("provider maps back");
    assert_eq!(back.pos, LspPosition::new(1, 3));
}

/// An unknown extension (no registered language row) must NOT build a
/// provider projection: never serve a non-script document to the
/// TypeScript provider.
#[test]
fn did_open_unknown_extension_builds_no_projection() {
    let host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig::default(),
    ));
    let registry = DocumentRegistry::new(Arc::clone(&host));

    let uri: Uri = "file:///x/notes.md".parse().expect("uri");
    let _ = registry.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "markdown".to_string(),
        version: 1,
        text: "# notes\n".to_string(),
    });

    assert!(
        registry.get_projection(&uri).is_none(),
        "an unknown-extension document must not get a provider projection"
    );
}

/// With no compiled IDE output (a cold caller), `is_jsx_for_canonical`
/// must fall back to the parse-level script dialect — a JS carrier is
/// `.jsx` from the start, never a `.tsx` guess that flips later.
#[test]
fn is_jsx_for_canonical_falls_back_to_parse_dialect_without_ide_compile() {
    let host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig::default(),
    ));
    let registry = DocumentRegistry::new(Arc::clone(&host));

    // A no-lang Svelte component: the parse reports the JS dialect.
    let js_svelte = "<script>\nlet msg = 'hi';\n</script>\n<p>{msg}</p>";
    let _ = host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/x/JsComp.svelte".to_string()),
        input_id: "/x/JsComp.svelte".to_string(),
        source: Arc::from(js_svelte),
        file_language: verter_session::FileLanguage::svelte(),
        aliases: vec![],
    });
    let ts_vue = "<script setup lang=\"ts\">\nconst msg: string = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>";
    let _ = host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/x/TsComp.vue".to_string()),
        input_id: "/x/TsComp.vue".to_string(),
        source: Arc::from(ts_vue),
        file_language: verter_session::FileLanguage::vue(),
        aliases: vec![],
    });

    // No IDE compile ran — the parse-level dialect decides.
    let analysis = host.get_analysis("/x/JsComp.svelte");
    assert!(
        analysis.is_some(),
        "analysis must exist for the fallback to consult"
    );
    assert!(
        !analysis.unwrap().is_typescript,
        "a no-lang Svelte script is not TypeScript"
    );
    assert!(
        registry.is_jsx_for_canonical("/x/JsComp.svelte"),
        "a no-lang (JS) Svelte carrier is .jsx without an IDE compile"
    );
    assert!(
        !registry.is_jsx_for_canonical("/x/TsComp.vue"),
        "a lang=ts Vue carrier is .tsx without an IDE compile"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn optional_semantic_analysis_is_isolated_and_published_asynchronously() {
    let cases = [
        (
            "vue",
            "file:///workspace/App.vue",
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\nconst count = 1\n</script>\n<template><Child>{{ count }}</Child></template>",
        ),
        (
            "svelte",
            "file:///workspace/App.svelte",
            "<script lang=\"ts\">\nimport Child from './Child.svelte';\nconst count = 1;\n</script>\n<Child>{count}</Child>",
        ),
    ];

    for (language_id, uri_text, source) in cases {
        let projection_host = Arc::new(verter_session::VerterHost::new_standalone(
            verter_session::HostConfig {
                analysis_scope: Some(verter_semantic::analysis::AnalysisScope::IMPORTS),
                ..verter_session::HostConfig::default()
            },
        ));
        let registry = Arc::new(DocumentRegistry::new(projection_host));
        let uri: tower_lsp_server::ls_types::Uri = uri_text.parse().unwrap();
        let _ = registry.did_open(&tower_lsp_server::ls_types::TextDocumentItem {
            uri: uri.clone(),
            language_id: language_id.to_string(),
            version: 1,
            text: source.to_string(),
        });

        registry.schedule_semantic_analysis(&uri);
        tokio::task::yield_now().await;
        assert!(
            registry.semantic_host.read().is_none(),
            "disabled {language_id} enrichment must not construct the isolated semantic host"
        );
        assert!(
            registry.get_analysis(&uri).is_none(),
            "the {language_id} projection host must not lazily reconstruct semantic enrichment"
        );

        registry.set_semantic_analysis_enabled(true);
        registry.schedule_semantic_analysis(&uri);
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if registry
                    .get_analysis(&uri)
                    .is_some_and(|analysis| analysis.template.is_some())
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "background {language_id} semantic snapshot should publish without inline request computation"
            )
        });
    }
}
