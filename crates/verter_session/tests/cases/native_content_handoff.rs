use std::sync::Arc;

use verter_session::{
    hash_block_content, BlockContentAvailability, BlockContentOrigin, BlockContentQuery,
    BlockContentRefusal, BlockOverrideEntry, BlockOverrideRequest, CompileProfile, FileLanguage,
    HostConfig, HostError, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};

fn upsert(host: &VerterHost, id: &str, source: &str, file_language: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language,
            aliases: Vec::new(),
        })
        .unwrap();
}

fn query(canonical_id: &str, block_token: &impl std::fmt::Display) -> BlockContentQuery {
    BlockContentQuery {
        canonical_id: canonical_id.to_string(),
        block_token: block_token.to_string(),
        compile_profile: CompileProfile::default(),
        expected_basis_token: None,
    }
}

fn supplied_entry(request: &verter_session::PreprocessorRequest, code: &str) -> BlockOverrideEntry {
    BlockOverrideEntry {
        correlation_token: request.correlation_token.clone(),
        block_token: request.block_token.clone(),
        owner_revision: request.owner_revision.clone(),
        artifact_token: request.artifact_token.clone(),
        basis_token: request.basis_token.clone(),
        captured_echo: request.captured_echo.clone(),
        source_space_token: request.source_space_token.clone(),
        code: Arc::from(code),
        code_hash: hash_block_content(code),
        source_map: None,
        source_map_hash: None,
        dependencies: Vec::new(),
        diagnostics: Vec::new(),
        processor_identity: "test-provider".to_string(),
        processor_version: "0.0.0-test".to_string(),
        config_fingerprint: None,
    }
}

#[test]
fn native_external_style_reads_registered_vfs_content_and_parses_its_dialect() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let owner = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/App.vue".to_string(),
            source: Arc::from(
                "<template><div class=\"external-card\"/></template><style src=\"./theme.scss\" lang=\"scss\"></style>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let external = owner
        .external_source_requests
        .first()
        .expect("external request");
    upsert(
        &host,
        "/workspace/theme.scss",
        "$tone: red; .external-card { color: $tone; }",
        FileLanguage::script_ts(),
    );

    let content = host
        .get_block_content(query(&owner.canonical_id, &external.block_token))
        .expect("native external content");
    assert_eq!(
        content.availability,
        BlockContentAvailability::NativeAvailable
    );
    assert!(matches!(
        content.origin,
        Some(BlockContentOrigin::NativeVfs { .. })
    ));
    assert_eq!(
        content.content.as_deref(),
        Some("$tone: red; .external-card { color: $tone; }")
    );
    assert_ne!(
        content.source_space_token.as_str(),
        external.carrier_source_space_token.as_str()
    );

    let analysis = host.get_analysis(&owner.canonical_id).expect("analysis");
    let style = analysis.styles.first().expect("style");
    assert_eq!(
        style.content_availability,
        BlockContentAvailability::NativeAvailable
    );
    assert!(
        style.css.is_none(),
        "external CSS facts must fail closed: block-local spans would be read as carrier-absolute"
    );
    assert!(style.v_binds.is_empty());
    assert_eq!(
        style.source_space_token.as_deref(),
        Some(content.source_space_token.as_str())
    );
}

#[test]
fn external_style_dialect_is_inferred_from_resolved_specifier_extension() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/workspace/theme.scss",
        "$tone: red; .extension-dialect { color: $tone; }",
        FileLanguage::script_ts(),
    );
    let owner = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/App.vue".to_string(),
            source: Arc::from("<style src=\"./theme.scss\"></style>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let external = owner.external_source_requests.first().unwrap();
    let content = host
        .get_block_content(query(&owner.canonical_id, &external.block_token))
        .unwrap();
    assert_eq!(content.lang, "scss");
    assert_eq!(
        content.availability,
        BlockContentAvailability::NativeAvailable
    );
    let analysis = host.get_analysis(&owner.canonical_id).unwrap();
    assert!(
        analysis.styles[0].css.is_none(),
        "external style analysis must not publish block-local spans as carrier-absolute"
    );
}

#[test]
fn native_external_script_is_a_registered_parsed_vfs_artifact() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let owner = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/App.vue".to_string(),
            source: Arc::from("<script src=\"./logic.ts\" lang=\"ts\"></script>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let external = owner
        .external_source_requests
        .first()
        .expect("external request");
    upsert(
        &host,
        "/workspace/logic.ts",
        "import { ref } from 'vue'; const externalValue = ref<number>(42);",
        FileLanguage::script_ts(),
    );

    let content = host
        .get_block_content(query(&owner.canonical_id, &external.block_token))
        .expect("native external script content");
    assert_eq!(
        content.availability,
        BlockContentAvailability::NativeAvailable
    );
    assert!(matches!(
        content.origin,
        Some(BlockContentOrigin::NativeVfs { .. })
    ));
    assert_ne!(
        content.source_space_token.as_str(),
        external.carrier_source_space_token.as_str()
    );

    let analysis = host
        .get_analysis(&external.resolved_canonical_id)
        .expect("registered external script analysis");
    assert!(
        analysis
            .bindings
            .iter()
            .any(|binding| binding.name == "externalValue")
            && analysis.imports.iter().any(|import| import.source == "vue"),
        "the external script must be parsed under its own registered identity"
    );
}

#[test]
fn processed_external_request_carries_external_bytes_not_empty_carrier_span() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/workspace/view.pug",
        "main.external-pug-marker",
        FileLanguage::script_ts(),
    );
    let owner = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/App.vue".to_string(),
            source: Arc::from("<template src=\"./view.pug\" lang=\"pug\"></template>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let request = owner.preprocessor_requests.first().expect("Pug request");
    assert_eq!(request.content, "main.external-pug-marker");
    assert!(!request.source_space_token.is_empty());
}

#[test]
fn processed_classes_stay_typed_until_a_stamped_result_is_admitted() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/App.vue".to_string(),
            source: Arc::from(
                "<template lang=\"pug\">div hello</template><script lang=\"coffee\">answer = 42</script><i18n lang=\"yaml\">hello: world</i18n>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();

    assert_eq!(update.preprocessor_requests.len(), 3);
    for request in &update.preprocessor_requests {
        assert_eq!(
            request.availability,
            BlockContentAvailability::ProcessedContentRequired
        );
        assert!(!request.block_token.is_empty());
        assert!(!request.owner_revision.is_empty());
        assert!(!request.artifact_token.is_empty());
        assert!(!request.basis_token.is_empty());
        assert!(!request.correlation_token.is_empty());
        assert!(
            request.prior_basis_token.is_none(),
            "first resolution is tokenless before host capture"
        );
        assert_eq!(request.pre_capture_echo.prior_basis_token, None);
        assert_eq!(
            request.captured_echo.request, request.pre_capture_echo,
            "post-capture must wrap and echo the exact tokenless first phase"
        );
        assert_eq!(
            request.captured_echo.request.correlation_token,
            request.correlation_token
        );
        assert_eq!(
            request.captured_echo.request.block_token,
            request.block_token
        );
        assert_eq!(request.captured_echo.basis_token, request.basis_token);
        let pending = host
            .get_block_content(query(&update.canonical_id, &request.block_token))
            .expect("pending content");
        assert_eq!(
            pending.availability,
            BlockContentAvailability::ProcessedContentRequired
        );
    }

    let pug = &update.preprocessor_requests[0];
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id.clone(),
            compile_profile: CompileProfile::default(),
            overrides: vec![supplied_entry(pug, "<div>hello</div>")],
        })
        .expect("stamped result admitted");

    let supplied = host
        .get_block_content(query(&update.canonical_id, &pug.block_token))
        .expect("supplied content");
    assert_eq!(
        supplied.availability,
        BlockContentAvailability::SuppliedAvailable
    );
    assert!(matches!(
        supplied.origin,
        Some(BlockContentOrigin::SuppliedValidated { .. })
    ));
    assert_eq!(supplied.content.as_deref(), Some("<div>hello</div>"));

    let coffee = &update.preprocessor_requests[1];
    assert_eq!(
        host.get_block_content(query(&update.canonical_id, &coffee.block_token))
            .unwrap()
            .availability,
        BlockContentAvailability::ProcessedContentRequired,
        "admission is block-token scoped, never ordinal/broadcast"
    );
}

#[test]
fn captured_echo_mismatch_is_a_post_capture_terminal_without_content_admission() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Echo.vue".to_string(),
            source: Arc::from("<template lang=\"pug\">p echo</template>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let request = &update.preprocessor_requests[0];
    let mut entry = supplied_entry(request, "<p>echo</p>");
    entry.captured_echo.request.expected_language = "coffee".to_string();
    let error = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id.clone(),
            compile_profile: CompileProfile::default(),
            overrides: vec![entry],
        })
        .unwrap_err();
    assert!(matches!(
        error,
        HostError::BlockContentRefused(BlockContentRefusal::CorrelationMismatch)
    ));
    assert_eq!(
        host.get_block_content(query(&update.canonical_id, &request.block_token))
            .unwrap()
            .availability,
        BlockContentAvailability::ProcessedContentRequired
    );
    let replay = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id,
            compile_profile: CompileProfile::default(),
            overrides: vec![supplied_entry(request, "<p>echo</p>")],
        })
        .unwrap_err();
    assert!(matches!(
        replay,
        HostError::BlockContentRefused(BlockContentRefusal::CorrelationMismatch)
    ));
}

#[test]
fn supplied_validated_precedence_exposes_exactly_one_live_source() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/App.vue".to_string(),
            source: Arc::from("<template lang=\"pug\">p authored</template>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let request = update.preprocessor_requests.first().unwrap();

    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id.clone(),
            compile_profile: CompileProfile::default(),
            overrides: vec![supplied_entry(request, "<p>supplied</p>")],
        })
        .unwrap();
    let selected = host
        .get_block_content(query(&update.canonical_id, &request.block_token))
        .unwrap();

    assert_eq!(selected.content.as_deref(), Some("<p>supplied</p>"));
    assert!(matches!(
        selected.origin,
        Some(BlockContentOrigin::SuppliedValidated { .. })
    ));
    assert_eq!(
        selected.availability,
        BlockContentAvailability::SuppliedAvailable
    );
}

#[test]
fn native_external_scss_does_not_offer_a_supplied_output_request() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/workspace/theme.scss",
        ".native-only { color: red }",
        FileLanguage::script_ts(),
    );
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/App.vue".to_string(),
            source: Arc::from("<style src=\"./theme.scss\" lang=\"scss\"></style>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    assert!(
        update.preprocessor_requests.is_empty(),
        "native style dialects must not widen the supplied-output request surface"
    );
}

#[test]
fn supplied_template_runtime_compile_lowers_validated_bytes() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/App.vue".to_string(),
            source: Arc::from("<template lang=\"pug\">p authored-only</template>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let request = update.preprocessor_requests.first().unwrap();
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id.clone(),
            compile_profile: CompileProfile::default(),
            overrides: vec![supplied_entry(request, "<div>supplied-only</div>")],
        })
        .unwrap();

    let output = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(update.canonical_id),
            node_kind: Some(VirtualNodeKind::Template),
            compile_profile: CompileProfile::default(),
        })
        .expect("validated supplied template lowering");
    assert!(output.code.contains("supplied-only"));
    assert!(!output.code.contains("authored-only"));
}

#[test]
fn external_template_ide_compile_contains_selected_bytes() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/workspace/view.html",
        "<div>external-only</div>",
        FileLanguage::script_ts(),
    );
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/App.vue".to_string(),
            source: Arc::from("<template src=\"./view.html\"></template>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();

    let profile = CompileProfile::default();
    let compiled = host
        .ensure_ide_compiled(&update.canonical_id, &CompileProfile::default())
        .expect("IDE lowering includes selected template bytes");
    assert!(compiled, "external template must produce an IDE surface");
    let ide = host
        .get_ide(&update.canonical_id, &profile)
        .expect("compiled IDE surface");
    assert!(
        ide.code.contains("external-only"),
        "selected external template bytes are absent from the IDE surface:\n{}",
        ide.code
    );
}

/// @ai-generated - The host must preserve the compiler's typed refusal for an
/// external template whose carrier has only an Options API script.
#[test]
fn external_template_with_plain_script_has_no_host_ide_surface() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/workspace/view.html",
        "<div>{{ count }}</div>",
        FileLanguage::script_ts(),
    );
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/App.vue".to_string(),
            source: Arc::from(concat!(
                "<template src=\"./view.html\"></template>",
                "<script>export default { data: () => ({ count: 1 }) }</script>"
            )),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let profile = CompileProfile::default();

    let error = host
        .ensure_ide_compiled(&update.canonical_id, &profile)
        .expect_err("plain-script external-template IDE must fail closed");
    let HostError::CompileError(failure) = error else {
        panic!("unexpected refusal: {error:?}");
    };
    assert!(failure
        .diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| { diagnostic.code == "HOST_BLOCK_CONTENT_IDE_UNAVAILABLE" }));
    assert!(
        host.get_ide(&update.canonical_id, &profile).is_none(),
        "a refused compile must not publish broken TSX"
    );
}

#[test]
fn supplied_style_analysis_fails_closed_without_carrier_absolute_spans() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/App.vue".to_string(),
            source: Arc::from("<style lang=\"postcss\">.authored-only { color: red }</style>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let request = update.preprocessor_requests.first().unwrap();
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id.clone(),
            compile_profile: CompileProfile::default(),
            overrides: vec![supplied_entry(request, ".supplied-only { color: blue }")],
        })
        .unwrap();

    let analysis = host.get_analysis(&update.canonical_id).unwrap();
    let style = analysis.styles.first().expect("style analysis");
    assert_eq!(
        style.content_availability,
        BlockContentAvailability::SuppliedAvailable
    );
    assert!(
        style.css.is_none(),
        "supplied CSS facts must fail closed until LSP consumers understand their source space"
    );
    assert!(style.v_binds.is_empty());
    assert!(style.source_space_token.is_some());
}

#[test]
fn incompatible_inline_and_external_sources_are_conflict_not_two_live_paths() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/App.vue".to_string(),
            source: Arc::from("<style src=\"./theme.css\">.inline { color: red }</style>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let external = update
        .external_source_requests
        .first()
        .expect("external request");
    upsert(
        &host,
        "/workspace/theme.css",
        ".external { color: blue }",
        FileLanguage::script_ts(),
    );

    let selected = host
        .get_block_content(query(&update.canonical_id, &external.block_token))
        .unwrap();
    assert_eq!(selected.availability, BlockContentAvailability::Conflict);
    assert!(selected.content.is_none());
    assert!(selected.origin.is_none());

    let conflict_request = update
        .preprocessor_requests
        .iter()
        .find(|request| request.block_token.as_str() == external.block_token);
    assert!(
        conflict_request.is_none(),
        "a conflicting block must never mint an admission capability"
    );
}

#[test]
fn stale_or_hash_mismatched_results_refuse_without_cache_mutation() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let first = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/App.vue".to_string(),
            source: Arc::from("<template lang=\"pug\">p first</template>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let stale_request = first.preprocessor_requests.first().unwrap().clone();

    let mut bad_hash = supplied_entry(&stale_request, "<p>first</p>");
    bad_hash.code_hash = hash_block_content("different bytes");
    let error = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: first.canonical_id.clone(),
            compile_profile: CompileProfile::default(),
            overrides: vec![bad_hash],
        })
        .expect_err("hash mismatch must refuse");
    assert!(matches!(
        error,
        HostError::BlockContentRefused(BlockContentRefusal::CodeHashMismatch)
    ));
    assert_eq!(
        host.get_block_content(query(&first.canonical_id, &stale_request.block_token))
            .unwrap()
            .availability,
        BlockContentAvailability::ProcessedContentRequired
    );

    let stale_host = VerterHost::new_standalone(HostConfig::default());
    let stale_first = stale_host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Stale.vue".to_string(),
            source: Arc::from("<template lang=\"pug\">p first</template>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let stale_request = stale_first.preprocessor_requests.first().unwrap().clone();
    let second = stale_host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Stale.vue".to_string(),
            source: Arc::from("<template lang=\"pug\">p second</template>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let error = stale_host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: second.canonical_id.clone(),
            compile_profile: CompileProfile::default(),
            overrides: vec![supplied_entry(&stale_request, "<p>first</p>")],
        })
        .expect_err("old await result must refuse");
    assert!(matches!(
        error,
        HostError::BlockContentRefused(BlockContentRefusal::CorrelationSuperseded)
    ));
    let live_request = second.preprocessor_requests.first().unwrap();
    assert_eq!(
        stale_host
            .get_block_content(query(&second.canonical_id, &live_request.block_token))
            .unwrap()
            .availability,
        BlockContentAvailability::ProcessedContentRequired
    );
}

#[test]
fn invalid_or_hash_mismatched_maps_refuse_without_content_mutation() {
    fn pending_host(id: &str) -> (VerterHost, verter_session::HostUpdateResult) {
        let host = VerterHost::new_standalone(HostConfig::default());
        let update = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: id.to_string(),
                source: Arc::from("<template lang=\"pug\">p mapped</template>"),
                file_language: FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();
        (host, update)
    }

    for (suffix, map, map_hash, expected) in [
        (
            "invalid",
            "{not-json",
            hash_block_content("{not-json"),
            BlockContentRefusal::InvalidSourceMap,
        ),
        (
            "shape",
            "{}",
            hash_block_content("{}"),
            BlockContentRefusal::InvalidSourceMap,
        ),
        (
            "invalid-mappings",
            "{\"version\":3,\"sources\":[\"input.pug\"],\"names\":[],\"mappings\":\"!!!\"}",
            hash_block_content(
                "{\"version\":3,\"sources\":[\"input.pug\"],\"names\":[],\"mappings\":\"!!!\"}",
            ),
            BlockContentRefusal::InvalidSourceMap,
        ),
        (
            "multiple-input-spaces",
            "{\"version\":3,\"sources\":[\"input.pug\",\"other.pug\"],\"names\":[],\"mappings\":\"AAAA\"}",
            hash_block_content(
                "{\"version\":3,\"sources\":[\"input.pug\",\"other.pug\"],\"names\":[],\"mappings\":\"AAAA\"}",
            ),
            BlockContentRefusal::InvalidSourceMap,
        ),
        (
            "mismatched-input-bytes",
            "{\"version\":3,\"sources\":[\"input.pug\"],\"sourcesContent\":[\"p other\"],\"names\":[],\"mappings\":\"AAAA\"}",
            hash_block_content(
                "{\"version\":3,\"sources\":[\"input.pug\"],\"sourcesContent\":[\"p other\"],\"names\":[],\"mappings\":\"AAAA\"}",
            ),
            BlockContentRefusal::InvalidSourceMap,
        ),
        (
            "generated-position-out-of-bounds",
            "{\"version\":3,\"sources\":[\"input.pug\"],\"names\":[],\"mappings\":\"oGAAA\"}",
            hash_block_content(
                "{\"version\":3,\"sources\":[\"input.pug\"],\"names\":[],\"mappings\":\"oGAAA\"}",
            ),
            BlockContentRefusal::InvalidSourceMap,
        ),
        (
            "original-position-out-of-bounds",
            "{\"version\":3,\"sources\":[\"input.pug\"],\"names\":[],\"mappings\":\"AAUA\"}",
            hash_block_content(
                "{\"version\":3,\"sources\":[\"input.pug\"],\"names\":[],\"mappings\":\"AAUA\"}",
            ),
            BlockContentRefusal::InvalidSourceMap,
        ),
        (
            "hash",
            "{\"version\":3,\"sources\":[],\"names\":[],\"mappings\":\"\"}",
            hash_block_content("different map bytes"),
            BlockContentRefusal::SourceMapHashMismatch,
        ),
    ] {
        let (host, update) = pending_host(&format!("/workspace/{suffix}.vue"));
        let request = update.preprocessor_requests.first().unwrap();
        let mut entry = supplied_entry(request, "<p>mapped</p>");
        entry.source_map = Some(Arc::from(map));
        entry.source_map_hash = Some(map_hash);
        let error = host
            .apply_block_overrides(BlockOverrideRequest {
                canonical_id: update.canonical_id.clone(),
                compile_profile: CompileProfile::default(),
                overrides: vec![entry],
            })
            .expect_err("untrusted map must be refused");
        assert_eq!(
            error.to_string(),
            HostError::BlockContentRefused(expected).to_string()
        );
        assert_eq!(
            host.get_block_content(query(&update.canonical_id, &request.block_token))
                .unwrap()
                .availability,
            BlockContentAvailability::ProcessedContentRequired
        );
    }
}

#[test]
fn supplied_output_has_distinct_artifact_and_source_space_identity() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Identity.vue".to_string(),
            source: Arc::from("<template lang=\"pug\">p input</template>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let request = update.preprocessor_requests.first().unwrap();
    let mut entry = supplied_entry(request, "<p>output</p>");
    let map = "{\"version\":3,\"sources\":[\"input.pug\"],\"sourcesContent\":[\"p input\"],\"names\":[],\"mappings\":\"AAAA\"}";
    entry.source_map = Some(Arc::from(map));
    entry.source_map_hash = Some(hash_block_content(map));
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id.clone(),
            compile_profile: CompileProfile::default(),
            overrides: vec![entry],
        })
        .unwrap();

    let supplied = host
        .get_block_content(query(&update.canonical_id, &request.block_token))
        .unwrap();
    assert_eq!(
        supplied.availability,
        BlockContentAvailability::SuppliedAvailable
    );
    assert_ne!(supplied.source_space_token, request.source_space_token);
    assert_ne!(
        supplied.content_artifact_token.as_str(),
        request.artifact_token.as_str()
    );
    assert_eq!(supplied.source_map.as_deref(), Some(map));
    assert_eq!(supplied.source_spaces.len(), 2);
    assert_eq!(
        supplied.final_output_space.token,
        supplied.source_space_token
    );
    assert_eq!(
        supplied.composed_map.destination_space_token,
        supplied.source_space_token
    );
    assert_eq!(
        supplied.composed_map.declared_space_tokens,
        vec![request.source_space_token.clone()]
    );
    assert_eq!(supplied.immediate_maps, vec![supplied.composed_map.clone()]);
}

#[test]
fn external_revision_invalidates_supplied_precedence_without_hiding_native_bytes() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        "/workspace/view.pug",
        "p first-native",
        FileLanguage::script_ts(),
    );
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/ExternalProcessed.vue".to_string(),
            source: Arc::from("<template src=\"./view.pug\" lang=\"pug\"></template>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let request = update.preprocessor_requests.first().unwrap();
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id.clone(),
            compile_profile: CompileProfile::default(),
            overrides: vec![supplied_entry(request, "<p>supplied-old</p>")],
        })
        .unwrap();

    upsert(
        &host,
        "/workspace/view.pug",
        "p second-native",
        FileLanguage::script_ts(),
    );
    let selected = host
        .get_block_content(query(&update.canonical_id, &request.block_token))
        .unwrap();
    assert_eq!(
        selected.availability,
        BlockContentAvailability::ProcessedContentRequired
    );
    assert_eq!(selected.content.as_deref(), Some("p second-native"));
    assert!(matches!(
        selected.origin,
        Some(BlockContentOrigin::NativeVfs { .. })
    ));
}

#[test]
fn byte_identical_noop_upsert_returns_current_stamps_without_content_mutation() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let source = "<template lang=\"pug\">p stable</template>";
    let first = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Stable.vue".to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let second = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Stable.vue".to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    assert!(!second.changed);
    assert_ne!(
        first.preprocessor_requests[0].correlation_token,
        second.preprocessor_requests[0].correlation_token,
        "a new scheduler capture must not alias an older in-flight await"
    );
    assert_eq!(
        first.preprocessor_requests[0].content,
        second.preprocessor_requests[0].content
    );
}

#[test]
fn close_terminalizes_pending_handoffs_and_drops_content() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Closed.vue".to_string(),
            source: Arc::from("<template lang=\"pug\">p pending</template>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let request = update.preprocessor_requests[0].clone();
    host.close();
    let error = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id,
            compile_profile: CompileProfile::default(),
            overrides: vec![supplied_entry(&request, "<p>late</p>")],
        })
        .unwrap_err();
    assert!(matches!(
        error,
        HostError::BlockContentRefused(BlockContentRefusal::CorrelationClosed)
    ));
}

#[test]
fn availability_match_is_compile_time_exhaustive() {
    fn code(value: BlockContentAvailability) -> u8 {
        match value {
            BlockContentAvailability::NativeAvailable => 1,
            BlockContentAvailability::ProcessedContentRequired => 2,
            BlockContentAvailability::SuppliedAvailable => 3,
            BlockContentAvailability::Missing => 4,
            BlockContentAvailability::Conflict => 5,
            BlockContentAvailability::Stale => 6,
        }
    }

    assert_eq!(code(BlockContentAvailability::Stale), 6);
}
