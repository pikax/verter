//! A completed `BlockOverrideEntry` round-trip is a genuine, working provenance
//! for the plain-CSS-gated style cascade — not merely a documented intent.
//!
//! A `<style lang="postcss" scoped>` block is outside the five native dialects,
//! so the host requests external preprocessing for it. The supplied result must
//! reach Vue style transforms and publish real rewritten CSS.

use std::sync::Arc;

use verter_session::{
    hash_block_content, BlockOverrideEntry, BlockOverrideRequest, CompileProfile, FileLanguage,
    HostConfig, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
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
        supplied_provenance: Some("test-preprocessor".to_string()),
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
