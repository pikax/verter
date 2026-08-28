//! A completed `BlockOverrideEntry` round-trip is a genuine, working provenance
//! for the plain-CSS-gated style cascade — not merely a documented intent.
//!
//! A `<style lang="postcss" scoped>` block is outside the five native dialects,
//! so the host requests external preprocessing for it. The supplied result must
//! reach Vue style transforms and publish real rewritten CSS.

use std::sync::Arc;

use verter_session::{
    hash_block_content, BlockContentAvailability, BlockContentHashToken, BlockContentOrigin,
    BlockContentQuery, BlockOverrideEntry, BlockOverrideRequest, CompileProfile, FileLanguage,
    HostConfig, HostSeverity, PreprocessorDiagnostic, UpsertRequest, VerterHost, VirtualNodeKind,
    VirtualQuery,
};

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
        processor_identity: "test-preprocessor".to_string(),
        processor_version: "0.0.0-test".to_string(),
        config_fingerprint: None,
    }
}

/// Positive: a completed override of a postcss style block reaches scoped
/// rewrite. `postcss` is outside the five native dialects, so the host issues
/// a preprocessor request; the supplied plain CSS must still enter the
/// type-state-gated cascade.
#[test]
fn completed_block_override_round_trip_reaches_the_scoped_selector_stage() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Comp.vue".to_string(),
            source: Arc::from(
                "<template><div class=\"tone\">x</div></template>\
                 <style lang=\"postcss\" scoped>.tone { color: red; }</style>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let request = update
        .preprocessor_requests
        .first()
        .expect("the postcss style block needs external preprocessing");

    let preprocessed = ".tone { color: red; }";
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id.clone(),
            compile_profile: CompileProfile::default(),
            overrides: vec![supplied_entry(request, preprocessed)],
        })
        .unwrap();

    let style = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(update.canonical_id.clone()),
            node_kind: Some(VirtualNodeKind::Style { index: 0 }),
            compile_profile: CompileProfile::default(),
        })
        .unwrap_or_else(|error| {
            panic!(
                "the completed BlockOverrideEntry round-trip must publish a style node: {error:?}"
            )
        });
    assert!(
        style.code.contains("[data-v-"),
        "the supplied plain CSS must have reached the scoped-selector stage: {}",
        style.code
    );
    assert!(
        style.code.contains(".tone"),
        "the scoped selector must retain the supplied class: {}",
        style.code
    );
}

/// Positive: supplied CSS that still contains `v-bind()` is rewritten by the
/// type-state-gated cascade, not passed through as if preprocessing had already
/// lowered Vue constructs.
#[test]
fn completed_block_override_reaches_the_vue_style_transform() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Comp.vue".to_string(),
            source: Arc::from(
                "<template><div class=\"roundtrip\">x</div></template>\
                 <style lang=\"postcss\" scoped>.authored { color: v-bind(old); }</style>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("carrier upsert");
    let request = update
        .preprocessor_requests
        .first()
        .expect("the custom style dialect requires external preprocessing");
    let supplied = ".roundtrip { color: v-bind(theme); }";

    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id.clone(),
            compile_profile: CompileProfile::default(),
            overrides: vec![supplied_entry(request, supplied)],
        })
        .expect("completed override is admitted");

    let style = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(update.canonical_id),
            node_kind: Some(VirtualNodeKind::Style { index: 0 }),
            compile_profile: CompileProfile::default(),
        })
        .expect("completed override publishes a style output");

    assert!(style.code.contains(".roundtrip[data-v-"), "{}", style.code);
    assert!(style.code.contains("var(--"), "{}", style.code);
    assert!(!style.code.contains("v-bind("), "{}", style.code);
    assert!(!style.code.contains(".authored"), "{}", style.code);
    assert_ne!(style.code.as_ref(), supplied);
}

/// Negative control: authored plain CSS with no override reaches the same
/// scoped-selector stage. Both provenances land in the same product shape.
#[test]
fn authored_plain_css_reaches_the_same_scoped_selector_stage_without_any_override() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Plain.vue".to_string(),
            source: Arc::from(
                "<template><div class=\"tone\">x</div></template>\
                 <style scoped>.tone { color: red; }</style>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    assert!(
        update.preprocessor_requests.is_empty(),
        "plain CSS must never need external preprocessing"
    );

    let style = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(update.canonical_id.clone()),
            node_kind: Some(VirtualNodeKind::Style { index: 0 }),
            compile_profile: CompileProfile::default(),
        })
        .unwrap_or_else(|error| panic!("authored plain CSS must publish a style node: {error:?}"));
    assert!(style.code.contains("[data-v-"), "{}", style.code);
    assert!(style.code.contains(".tone"), "{}", style.code);
}

fn query(canonical_id: &str, block_token: &impl std::fmt::Display) -> BlockContentQuery {
    BlockContentQuery {
        canonical_id: canonical_id.to_string(),
        block_token: block_token.to_string(),
        compile_profile: CompileProfile::default(),
        expected_basis_token: None,
    }
}

/// Positive round trip: construct a real preprocessor request (a `<style
/// lang="styl">` block — Stylus is external until preprocessed), admit a
/// `BlockOverrideEntry` with all 6 fields populated with distinct real values,
/// and assert every field survives the round trip distinctly at
/// `BlockContentOrigin::SuppliedValidated`.
#[test]
fn six_field_preprocessor_contract_survives_the_round_trip_distinctly() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/RoundTrip.vue".to_string(),
            source: Arc::from("<style lang=\"styl\">.round-trip\n  color: red\n</style>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let request = update
        .preprocessor_requests
        .first()
        .expect("styl preprocessor request");

    let code: Arc<str> = Arc::from(".round-trip { color: red; }");
    let code_hash = hash_block_content(&code);
    let map = "{\"version\":3,\"sources\":[\"input.styl\"],\"sourcesContent\":\
               [\".round-trip\\n  color: red\\n\"],\"names\":[],\"mappings\":\"AAAA\"}";
    let source_map: Arc<str> = Arc::from(map);
    let source_map_hash = hash_block_content(&source_map);
    let dependencies = vec![
        "/workspace/_mixins.styl".to_string(),
        "/workspace/_vars.styl".to_string(),
    ];
    let diagnostics = vec![PreprocessorDiagnostic {
        severity: HostSeverity::Warning,
        message: "unused variable $legacy-tone".to_string(),
        line: Some(3),
        column: Some(5),
    }];
    let processor_identity = "stylus".to_string();
    let processor_version = "0.63.0".to_string();
    let config_fingerprint =
        BlockContentHashToken::parse_untrusted("stylus-config-fingerprint-v1").unwrap();

    let entry = BlockOverrideEntry {
        correlation_token: request.correlation_token.clone(),
        block_token: request.block_token.clone(),
        owner_revision: request.owner_revision.clone(),
        artifact_token: request.artifact_token.clone(),
        basis_token: request.basis_token.clone(),
        captured_echo: request.captured_echo.clone(),
        source_space_token: request.source_space_token.clone(),
        code: code.clone(),
        code_hash: code_hash.clone(),
        source_map: Some(source_map.clone()),
        source_map_hash: Some(source_map_hash),
        dependencies: dependencies.clone(),
        diagnostics: diagnostics.clone(),
        processor_identity: processor_identity.clone(),
        processor_version: processor_version.clone(),
        config_fingerprint: Some(config_fingerprint.clone()),
    };

    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id.clone(),
            compile_profile: CompileProfile::default(),
            overrides: vec![entry],
        })
        .expect("six-field supplied result admitted");

    let content = host
        .get_block_content(query(&update.canonical_id, &request.block_token))
        .expect("supplied content");
    assert_eq!(
        content.availability,
        BlockContentAvailability::SuppliedAvailable
    );
    assert_eq!(content.content.as_deref(), Some(&*code));
    assert_eq!(content.source_map.as_deref(), Some(&*source_map));
    assert_eq!(content.content_hash, Some(code_hash));

    let Some(BlockContentOrigin::SuppliedValidated {
        dependencies: got_dependencies,
        diagnostics: got_diagnostics,
        processor_identity: got_processor_identity,
        processor_version: got_processor_version,
        config_fingerprint: got_config_fingerprint,
    }) = content.origin
    else {
        panic!(
            "expected SuppliedValidated origin, got {:?}",
            content.origin
        );
    };

    assert_eq!(got_dependencies, dependencies);
    assert_eq!(got_processor_identity, processor_identity);
    assert_eq!(got_processor_version, processor_version);
    assert_ne!(got_processor_identity, got_processor_version);
    assert_eq!(got_config_fingerprint, Some(config_fingerprint));
    assert_eq!(got_diagnostics.len(), 1);
    assert_eq!(got_diagnostics[0].severity, HostSeverity::Warning);
    assert_eq!(got_diagnostics[0].message, diagnostics[0].message);
    assert_eq!(got_diagnostics[0].line, Some(3));
    assert_eq!(got_diagnostics[0].column, Some(5));
}

/// The zero-value case is distinct too: an entry with no dependencies, no
/// diagnostics, and no configuration fingerprint round-trips as genuinely
/// empty/absent — not as some fabricated placeholder.
#[test]
fn empty_optional_fields_round_trip_as_genuinely_absent() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Empty.vue".to_string(),
            source: Arc::from("<style lang=\"styl\">.empty-fields\n  color: blue\n</style>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let request = update
        .preprocessor_requests
        .first()
        .expect("styl preprocessor request");

    let code: Arc<str> = Arc::from(".empty-fields { color: blue; }");
    let entry = BlockOverrideEntry {
        correlation_token: request.correlation_token.clone(),
        block_token: request.block_token.clone(),
        owner_revision: request.owner_revision.clone(),
        artifact_token: request.artifact_token.clone(),
        basis_token: request.basis_token.clone(),
        captured_echo: request.captured_echo.clone(),
        source_space_token: request.source_space_token.clone(),
        code: code.clone(),
        code_hash: hash_block_content(&code),
        source_map: None,
        source_map_hash: None,
        dependencies: Vec::new(),
        diagnostics: Vec::new(),
        processor_identity: "stylus".to_string(),
        processor_version: "0.63.0".to_string(),
        config_fingerprint: None,
    };

    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id.clone(),
            compile_profile: CompileProfile::default(),
            overrides: vec![entry],
        })
        .expect("supplied result with empty optional fields admitted");

    let content = host
        .get_block_content(query(&update.canonical_id, &request.block_token))
        .expect("supplied content");
    let Some(BlockContentOrigin::SuppliedValidated {
        dependencies,
        diagnostics,
        config_fingerprint,
        ..
    }) = content.origin
    else {
        panic!(
            "expected SuppliedValidated origin, got {:?}",
            content.origin
        );
    };
    assert!(dependencies.is_empty());
    assert!(diagnostics.is_empty());
    assert_eq!(config_fingerprint, None);
}
