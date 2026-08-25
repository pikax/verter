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

    /// The `get_ide` SLOW path (host cache miss → recompile) must not install
    /// a mapper — nor return output — for a revision the document no longer
    /// holds (TE-C-15 window b: unfenced `get_mut` install).
    #[test]
    fn get_ide_slow_path_discards_output_when_source_moves_mid_compile() {
        let (registry, uri) = projectionless_registry();
        // Commit revision B: the did_change commit compiles nothing, so the
        // host's IDE cache misses and `get_ide` must take its SLOW path.
        let _ = registry.did_change(&uri, 2, REVISION_B);
        assert!(registry.get_projection(&uri).is_none());
        // While the slow-path compile runs, revision A re-commits as v3.
        registry.set_after_compile_hook(Box::new(|registry, uri| {
            let _ = registry.did_change(uri, 3, REVISION_A);
        }));

        let response = registry.get_ide(&uri);

        assert!(
            registry
                .get(&uri)
                .is_some_and(|doc| doc.source.as_ref() == REVISION_A),
            "the interleaved edit must commit the newest revision"
        );
        assert!(
            response.is_none(),
            "the superseded compile's retained IDE output must be discarded"
        );
        assert!(
            registry.get_projection(&uri).is_none(),
            "the superseded compile's mapper must not be installed over the newer revision"
        );
    }

    /// The `get_ide` FAST path (host cache hit) reads the host cache with no
    /// lock held; an edit committed in the read→install window must neither
    /// install the stale mapper nor return the stale IDE output (R2-B-01).
    #[test]
    fn get_ide_fast_path_discards_cached_output_when_source_moves_before_install() {
        let (registry, uri) = projectionless_registry();
        // The host cache still holds revision A's IDE surface (did_open
        // compiled it), so `get_ide` takes its FAST path. Revision B commits
        // in the read→install window.
        registry.set_before_projection_install_hook(Box::new(|registry, uri| {
            let _ = registry.did_change(uri, 2, REVISION_B);
        }));

        let response = registry.get_ide(&uri);

        assert!(
            registry
                .get(&uri)
                .is_some_and(|doc| doc.source.as_ref() == REVISION_B),
            "the interleaved edit must commit revision B"
        );
        assert!(
            response.is_none(),
            "revision A's cached IDE output must be discarded, not returned \
             as revision B's surface"
        );
        assert!(
            registry.get_projection(&uri).is_none(),
            "revision A's mapper must not be installed over revision B"
        );
    }

    /// `install_missing_carrier_projection` reads the host cache with no
    /// compile; an edit committed in the read→install window must reject the
    /// now-stale mapper (TE-C-15 window b).
    #[test]
    fn install_missing_projection_rejects_a_mapper_from_a_superseded_revision() {
        let (registry, uri) = projectionless_registry();
        let canonical = uri_to_canonical_id(&uri);
        // The host cache still holds revision A's IDE surface; revision B
        // commits in the read→install window.
        registry.set_before_projection_install_hook(Box::new(|registry, uri| {
            let _ = registry.did_change(uri, 2, REVISION_B);
        }));

        registry.install_missing_carrier_projection(&canonical);

        assert!(
            registry
                .get(&uri)
                .is_some_and(|doc| doc.source.as_ref() == REVISION_B),
            "the interleaved edit must commit revision B"
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
                        arguments: Vec::new(),
                        span: verter_span::Span::new(0, 1),
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

        // Disabled enrichment spawns NOTHING — the absent task is the proof,
        // so nothing has to be waited for or yielded past.
        assert!(
            registry.schedule_semantic_analysis_for_test(&uri).is_none(),
            "disabled {language_id} enrichment must not spawn a semantic task"
        );
        assert!(
            registry.semantic_host.read().is_none(),
            "disabled {language_id} enrichment must not construct the isolated semantic host"
        );
        assert!(
            registry.get_analysis(&uri).is_none(),
            "the {language_id} projection host must not lazily reconstruct semantic enrichment"
        );

        registry.set_semantic_analysis_enabled(true);
        // Subscribe BEFORE scheduling: the publication receipt is the wake,
        // not a yield loop over `get_analysis`. Joining the task itself would
        // also prove completion, but the receipt is what production consumers
        // use, so the test settles the same way they do.
        let mut ready = registry.subscribe_semantic_ready();
        registry
            .schedule_semantic_analysis_for_test(&uri)
            .expect("enabled enrichment must spawn a semantic task");
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                let event = ready
                    .recv()
                    .await
                    .expect("the semantic-ready channel stays open");
                if event.uri == uri.as_str() {
                    return;
                }
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "background {language_id} semantic snapshot should publish without inline request computation"
            )
        });
        assert!(
            registry
                .get_analysis(&uri)
                .is_some_and(|analysis| analysis.template.is_some()),
            "the {language_id} publication receipt must be backed by a stored template analysis"
        );
        let feature = registry
            .feature_snapshot(&uri)
            .expect("carrier feature snapshot");
        let semantic = feature
            .analysis()
            .expect("semantic enrichment must publish into the same snapshot");
        assert_eq!(semantic.document_revision(), feature.document_revision());
        assert_eq!(
            semantic.structure().artifact_id(),
            feature.structure().artifact_id()
        );
        assert!(
            Arc::ptr_eq(
                semantic.structure().envelope(),
                feature.structure().envelope()
            ),
            "semantic publication must retain the document's exact sealed envelope"
        );
        let semantic_host = registry
            .semantic_host
            .read()
            .clone()
            .expect("semantic host");
        assert_eq!(
            semantic.semantic_host_revision(),
            semantic_host
                .registered_source_revision_token(&uri_to_canonical_id(&uri))
                .expect("semantic Source-stage revision token")
        );
        assert_ne!(
            semantic.semantic_host_revision().host_instance,
            feature.projection_host_revision().host_instance,
            "projection and semantic hosts retain distinct identity domains"
        );
    }
}

fn semantic_test_registry() -> Arc<DocumentRegistry> {
    let projection_host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig {
            analysis_scope: Some(verter_semantic::analysis::AnalysisScope::IMPORTS),
            ..verter_session::HostConfig::default()
        },
    ));
    let registry = Arc::new(DocumentRegistry::new(projection_host));
    registry.set_semantic_analysis_enabled(true);
    registry
}

const SEMANTIC_REVISION_A: &str =
    "<script setup lang=\"ts\">\nconst value = 'a'\n</script>\n<template>{{ value }}</template>";
const SEMANTIC_REVISION_B: &str =
    "<script setup lang=\"ts\">\nconst value = 'b'\n</script>\n<template>{{ value }}</template>";

/// Production and tests share the 750ms quiet window — the sleep is no
/// longer compiled out under `cfg(test)`, so a test drives the same
/// scheduler topology production does, on a paused clock.
///
/// The discriminator is the ISOLATED SEMANTIC HOST, not the published
/// analysis. `semantic_host()` is constructed by the task itself, the
/// first thing it does after its sleep returns, so its absence at
/// `window - ε` is proof that no work has begun. "Analysis not published
/// yet" would be true anyway while the blocking upsert runs, and the
/// revision-discard half below does not need the window at all — neither
/// would notice the sleep being compiled out. Verified: putting the sleep
/// back behind `cfg(not(test))` turns the `window - ε` assertion red.
#[tokio::test(start_paused = true)]
async fn semantic_quiet_window_discards_stale_revision_at_the_bound() {
    let registry = semantic_test_registry();
    let uri: Uri = "file:///workspace/Quiet.vue".parse().unwrap();
    let _ = registry.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: SEMANTIC_REVISION_A.to_string(),
    });
    assert!(
        registry.semantic_host.read().is_none(),
        "test setup: no semantic host may exist before the task is scheduled"
    );
    // Arm interest BEFORE spawning: the task fires this receipt as its first
    // act, so awaiting it proves the task RAN and armed its quiet-window
    // sleep. Without it the absence assertions below could hold simply
    // because the task was never polled — vacuous rather than causal.
    let armed = registry.semantic_task_armed.notified();
    tokio::pin!(armed);
    armed.as_mut().enable();
    let handle = registry
        .schedule_semantic_analysis_for_test(&uri)
        .expect("semantic task");
    armed.await;

    tokio::time::advance(
        super::SEMANTIC_ANALYSIS_QUIET_WINDOW - std::time::Duration::from_millis(1),
    )
    .await;
    tokio::task::yield_now().await;
    assert!(
        registry.semantic_host.read().is_none(),
        "the semantic task must still be inside its quiet window at window - ε — \
         constructing the isolated host is the first thing it does once the sleep \
         returns, so a host here means the production sleep was skipped"
    );
    assert!(
        registry.get_analysis(&uri).is_none(),
        "semantic enrichment must not publish before the production quiet window"
    );

    let _ = registry.did_change(&uri, 2, SEMANTIC_REVISION_B);
    tokio::time::advance(std::time::Duration::from_millis(1)).await;
    handle.await.expect("semantic task joins");
    assert!(
        registry.get_analysis(&uri).is_none(),
        "a revision captured before the quiet window must be discarded after an edit inside it"
    );
    let current = registry
        .feature_snapshot(&uri)
        .expect("edited feature snapshot");
    assert_eq!(current.client_version(), 2);
    assert_eq!(current.source(), SEMANTIC_REVISION_B);
}

#[tokio::test(flavor = "multi_thread")]
async fn in_flight_semantic_completion_is_rejected_after_edit() {
    let registry = semantic_test_registry();
    let uri: Uri = "file:///workspace/Edit.vue".parse().unwrap();
    let _ = registry.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: SEMANTIC_REVISION_A.to_string(),
    });
    let captured = registry
        .feature_snapshot(&uri)
        .expect("captured feature snapshot");

    registry.set_before_semantic_publish_hook_for_test(Box::new(|registry, uri| {
        let _ = registry.did_change(uri, 2, SEMANTIC_REVISION_B);
        Some(
            registry
                .feature_snapshot(uri)
                .expect("edited feature snapshot inside publication seam")
                .structure()
                .clone(),
        )
    }));
    registry
        .schedule_semantic_analysis_for_test(&uri)
        .expect("semantic task")
        .await
        .expect("semantic task joins");

    let current = registry
        .feature_snapshot(&uri)
        .expect("current feature snapshot");
    assert_ne!(captured.document_revision(), current.document_revision());
    assert_eq!(current.client_version(), 2);
    assert_eq!(current.source(), SEMANTIC_REVISION_B);
    assert!(
        current.analysis().is_none(),
        "the stale completion must never attach to the edited document"
    );
    assert!(
        registry
            .cached_semantic_analysis(&uri_to_canonical_id(&uri))
            .is_none(),
        "the stale completion must not enter the semantic snapshot cache"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn committed_edit_invalidates_post_admission_semantic_publication() {
    let registry = semantic_test_registry();
    let uri: Uri = "file:///workspace/PostAdmissionEdit.vue".parse().unwrap();
    let canonical_id = uri_to_canonical_id(&uri);
    let _ = registry.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: SEMANTIC_REVISION_A.to_string(),
    });
    let mut ready = registry.subscribe_semantic_ready();

    let (early_window_reached_tx, early_window_reached_rx) = std::sync::mpsc::channel();
    let (resume_change_tx, resume_change_rx) = std::sync::mpsc::channel();
    registry.set_after_early_semantic_invalidation_window_hook_for_test(Box::new(move |_, _| {
        early_window_reached_tx
            .send(())
            .expect("test observes the former early invalidation window");
        resume_change_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("semantic admission releases the edit");
    }));

    let (reacquire_attempted_tx, reacquire_attempted_rx) = std::sync::mpsc::channel();
    let publication_canonical_id = canonical_id.clone();
    // The publication receipt is sent immediately AFTER the snapshot insert,
    // so blocking on it proves the cache write landed. This hook runs on the
    // plain `edit` thread, never inside the runtime, so a blocking receive is
    // legal here — and it is exact, unlike spinning on `contains_key`.
    let mut publication_receipt = registry.subscribe_semantic_ready();
    registry.set_before_change_document_reacquire_hook_for_test(Box::new(move |_, _| {
        reacquire_attempted_tx
            .send(())
            .expect("test observes the document shard reacquire attempt");
        loop {
            let published = publication_receipt
                .blocking_recv()
                .expect("old semantic publication reaches the cache");
            if published.canonical_id == publication_canonical_id {
                return;
            }
        }
    }));

    let edit_registry = Arc::clone(&registry);
    let edit_uri = uri.clone();
    let edit = std::thread::spawn(move || {
        let _ = edit_registry.did_change(&edit_uri, 2, SEMANTIC_REVISION_B);
    });
    early_window_reached_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("edit reaches the former early invalidation window");

    registry.set_after_semantic_admission_hook_for_test(Box::new(move |_, _| {
        resume_change_tx
            .send(())
            .expect("release edit after semantic admission");
        reacquire_attempted_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("edit reaches the blocked document shard reacquire");
    }));
    registry
        .schedule_semantic_analysis_for_test(&uri)
        .expect("semantic task")
        .await
        .expect("semantic task joins");
    edit.join().expect("edit thread joins");

    let current = registry
        .feature_snapshot(&uri)
        .expect("edited feature snapshot");
    assert_eq!(current.client_version(), 2);
    assert_eq!(current.source(), SEMANTIC_REVISION_B);
    assert!(
        current.analysis().is_none(),
        "the old semantic result must not attach to the edited document"
    );
    let published = ready
        .try_recv()
        .expect("the forced old task publishes ready");
    assert_eq!(published.canonical_id, canonical_id);
    assert_eq!(published.version, 1);
    assert!(
        registry.cached_semantic_analysis(&canonical_id).is_none(),
        "the committed edit must invalidate the old ready publication"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn admitted_semantic_publication_is_atomic_with_edit_invalidation() {
    let registry = semantic_test_registry();
    let uri: Uri = "file:///workspace/AtomicSemanticPublication.vue"
        .parse()
        .unwrap();
    let canonical_id = uri_to_canonical_id(&uri);
    let _ = registry.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: SEMANTIC_REVISION_A.to_string(),
    });
    let mut ready = registry.subscribe_semantic_ready();

    let (early_window_reached_tx, early_window_reached_rx) = std::sync::mpsc::channel();
    let (resume_change_tx, resume_change_rx) = std::sync::mpsc::channel();
    registry.set_after_early_semantic_invalidation_window_hook_for_test(Box::new(move |_, _| {
        early_window_reached_tx
            .send(())
            .expect("test observes the former early invalidation window");
        resume_change_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("semantic admission releases the edit");
    }));

    let (reacquire_attempted_tx, reacquire_attempted_rx) = std::sync::mpsc::channel();
    registry.set_before_change_document_reacquire_hook_for_test(Box::new(move |_, _| {
        reacquire_attempted_tx
            .send(())
            .expect("test observes the document shard reacquire attempt");
    }));

    let (edit_committed_tx, edit_committed_rx) = std::sync::mpsc::channel();
    let edit_registry = Arc::clone(&registry);
    let edit_uri = uri.clone();
    let edit = std::thread::spawn(move || {
        let _ = edit_registry.did_change(&edit_uri, 2, SEMANTIC_REVISION_B);
        let _ = edit_committed_tx.send(());
    });
    early_window_reached_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("edit reaches the former early invalidation window");

    registry.set_after_semantic_admission_hook_for_test(Box::new(move |_, _| {
        resume_change_tx
            .send(())
            .expect("release edit after semantic admission");
        reacquire_attempted_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("edit reaches the document shard reacquire attempt");
    }));
    let edit_committed_before_publication = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed_early_commit = Arc::clone(&edit_committed_before_publication);
    registry.set_before_semantic_cache_publication_hook_for_test(Box::new(move |_, _| {
        if edit_committed_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .is_ok()
        {
            observed_early_commit.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }));

    registry
        .schedule_semantic_analysis_for_test(&uri)
        .expect("semantic task")
        .await
        .expect("semantic task joins");
    edit.join().expect("edit thread joins");

    let current = registry
        .feature_snapshot(&uri)
        .expect("edited feature snapshot");
    assert_eq!(current.client_version(), 2);
    assert_eq!(current.source(), SEMANTIC_REVISION_B);
    assert!(current.analysis().is_none());
    assert!(
        registry.cached_semantic_analysis(&canonical_id).is_none(),
        "revision-A cache publication must not survive revision-B commit and invalidation"
    );
    assert!(
        !edit_committed_before_publication.load(std::sync::atomic::Ordering::SeqCst),
        "revision-B commit and invalidation must not split revision-A admission from publication"
    );
    let published = ready
        .try_recv()
        .expect("the admitted semantic task publishes ready before the edit commits");
    assert_eq!(published.canonical_id, canonical_id);
    assert_eq!(published.version, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn identical_text_version_close_reopen_rejects_old_semantic_completion() {
    let registry = semantic_test_registry();
    let uri: Uri = "file:///workspace/Reopen.vue".parse().unwrap();
    let item = TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 7,
        text: SEMANTIC_REVISION_A.to_string(),
    };
    let _ = registry.did_open(&item);
    let captured = registry
        .feature_snapshot(&uri)
        .expect("captured feature snapshot");
    let reopen_item = item.clone();

    registry.set_before_semantic_publish_hook_for_test(Box::new(move |registry, uri| {
        registry.did_close(uri);
        let _ = registry.did_open(&reopen_item);
        Some(
            registry
                .feature_snapshot(uri)
                .expect("reopened feature snapshot inside publication seam")
                .structure()
                .clone(),
        )
    }));
    registry
        .schedule_semantic_analysis_for_test(&uri)
        .expect("semantic task")
        .await
        .expect("semantic task joins");

    let reopened = registry
        .feature_snapshot(&uri)
        .expect("reopened feature snapshot");
    assert_ne!(captured.document_revision(), reopened.document_revision());
    assert_eq!(reopened.client_version(), captured.client_version());
    assert_eq!(reopened.source(), captured.source());
    assert_ne!(
        reopened.structure().artifact_id(),
        captured.structure().artifact_id(),
        "reopen advances the projection host's source identity"
    );
    assert!(
        reopened.analysis().is_none(),
        "the previous lifetime's completion must never attach after reopen"
    );
    assert!(
        registry
            .cached_semantic_analysis(&uri_to_canonical_id(&uri))
            .is_none(),
        "the previous lifetime's completion must not enter the semantic snapshot cache"
    );
}

/// TE-C-15 window a: after `did_change` commits the document entry, the stale
/// semantic snapshot must never be observable alongside the new entry — the
/// invalidation happens inside the same shard mutation, not after the guard
/// drops.
#[tokio::test(flavor = "multi_thread")]
async fn did_change_commit_window_never_exposes_a_stale_semantic_snapshot() {
    let registry = semantic_test_registry();
    let uri: Uri = "file:///workspace/CommitWindow.vue".parse().unwrap();
    let canonical_id = uri_to_canonical_id(&uri);
    let _ = registry.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: SEMANTIC_REVISION_A.to_string(),
    });
    registry
        .schedule_semantic_analysis_for_test(&uri)
        .expect("semantic task")
        .await
        .expect("semantic task joins");
    assert!(
        registry.cached_semantic_analysis(&canonical_id).is_some(),
        "precondition: the opened revision's semantic snapshot is published"
    );

    let observed: Arc<std::sync::Mutex<Option<bool>>> = Arc::new(std::sync::Mutex::new(None));
    let hook_observed = Arc::clone(&observed);
    let hook_canonical = canonical_id.clone();
    registry.set_after_change_commit_hook(Box::new(move |registry, _| {
        *hook_observed.lock().unwrap() =
            Some(registry.cached_semantic_analysis(&hook_canonical).is_some());
    }));
    let _ = registry.did_change(&uri, 2, SEMANTIC_REVISION_B);

    assert_eq!(
        *observed.lock().unwrap(),
        Some(false),
        "a reader observing the committed entry must never read the stale semantic snapshot"
    );
    assert!(registry.cached_semantic_analysis(&canonical_id).is_none());
}

/// Unit-proves the shared primitive both destructive-reload sites
/// (`resync_background_carrier_file`, the bootstrap branch in
/// `sync_imported_carrier_api_lightweight`) depend on to avoid substituting
/// disk content for an open document's unsaved edits.
///
/// `VerterHost::remove` clears the workspace overlay (via
/// `FilesystemWorkspace::notify_delete`); a bare re-read after that falls
/// through to disk. `reestablish_host_overlay_from_open_buffer` must put the
/// open document's OWN live buffer back as the overlay BEFORE anything reads
/// through the host again.
#[test]
fn reestablish_host_overlay_from_open_buffer_restores_the_live_buffer_over_disk() {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace_dir = temp.path().join("workspace");
    std::fs::create_dir_all(workspace_dir.join("src")).expect("workspace source dir");
    const SOURCE_A: &str = "<script setup lang=\"ts\">\nconst msg = 'disk-a'\n</script>\n\
                             <template><div>{{ msg }}</div></template>\n";
    const SOURCE_B: &str = "<script setup lang=\"ts\">\nconst msg = 'buffer-b'\n</script>\n\
                             <template><div>{{ msg }}</div></template>\n";
    std::fs::write(workspace_dir.join("src/App.vue"), SOURCE_A).expect("write disk source A");

    let workspace_root = crate::test_utils::canonical_test_path(&workspace_dir);
    let canonical_id = format!("{workspace_root}/src/App.vue");
    let uri: Uri = format!("file://{canonical_id}").parse().expect("uri");

    let ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let host = Arc::new(verter_session::VerterHost::new(
        verter_session::HostConfig::default(),
        ws,
    ));
    let registry = DocumentRegistry::new(Arc::clone(&host));

    let _ = registry.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: SOURCE_B.to_string(),
    });
    assert_eq!(
        host.get_source(&canonical_id).as_deref(),
        Some(SOURCE_B),
        "precondition: the open document's overlay serves its own buffer"
    );

    host.remove(&canonical_id);
    assert!(
        host.workspace_read()
            .read_file(&canonical_id)
            .as_deref()
            .is_some_and(|content| content.contains("disk-a")),
        "precondition: the raw workspace read now falls through to disk \
         (the overlay `remove` cleared)"
    );

    let reestablished = registry.reestablish_host_overlay_from_open_buffer(&canonical_id);
    assert!(
        reestablished,
        "the open document's overlay must be re-establishable after remove"
    );
    assert_eq!(
        host.get_source(&canonical_id).as_deref(),
        Some(SOURCE_B),
        "after re-establishing, the host must serve the open document's OWN \
         live buffer again, never disk"
    );

    // A closed canonical id has no buffer to re-establish from — a no-op.
    let closed_id = format!("{workspace_root}/src/NotOpen.vue");
    assert!(!registry.reestablish_host_overlay_from_open_buffer(&closed_id));
}
