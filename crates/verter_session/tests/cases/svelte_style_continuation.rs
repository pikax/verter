//! A completed external preprocessing result the host admits for a Svelte
//! `<style>` block is the only body that block's scoping reads, through the
//! public virtual-file boundary: fresh and edit-revert compiles agree, and a
//! result that no longer describes its block publishes nothing from it.

use std::sync::Arc;

use verter_session::{
    hash_block_content, BlockOverrideEntry, BlockOverrideRequest, CompileProfile, FileLanguage,
    HostConfig, PreprocessorRequest, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};

const INPUT_ID: &str = "/workspace/Card.svelte";
const SOURCE: &str =
    "<div class=\"card\">x</div>\n<style lang=\"scss\">$tone: red;\n.card { color: $tone; }</style>\n";
const EDITED: &str =
    "<div class=\"card\">x</div>\n<style lang=\"scss\">$tone: blue;\n.card { color: $tone; }</style>\n";
/// The preprocessor's output for [`SOURCE`]'s block.
const PRODUCED: &str = ".card { color: red; }";

fn upsert(
    host: &VerterHost,
    canonical_id: Option<String>,
    source: &str,
) -> (String, PreprocessorRequest) {
    let update = host
        .upsert(UpsertRequest {
            canonical_id,
            input_id: INPUT_ID.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::svelte(),
            aliases: Vec::new(),
        })
        .expect("svelte upsert");
    let request = update
        .preprocessor_requests
        .first()
        .expect("scss needs external preprocessing")
        .clone();
    (update.canonical_id, request)
}

fn admit(host: &VerterHost, canonical_id: &str, request: &PreprocessorRequest, code: &str) {
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: canonical_id.to_string(),
            compile_profile: CompileProfile::default(),
            overrides: vec![BlockOverrideEntry {
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
                processor_identity: "sass".to_string(),
                processor_version: "1.77.0".to_string(),
                config_fingerprint: None,
            }],
        })
        .expect("completed result is admitted");
}

fn virtual_file(
    host: &VerterHost,
    canonical_id: &str,
    node_kind: VirtualNodeKind,
) -> Option<String> {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical_id.to_string()),
        node_kind: Some(node_kind),
        compile_profile: CompileProfile::default(),
    })
    .ok()
    .map(|response| response.code.to_string())
}

/// Compile [`SOURCE`] with its produced body admitted, returning the
/// published stylesheet and main module.
fn compiled_with_supplied_result(host: &VerterHost, canonical_id: &str) -> (String, String) {
    let css = virtual_file(host, canonical_id, VirtualNodeKind::Style { index: 0 })
        .expect("a continued block compiles");
    let main = virtual_file(host, canonical_id, VirtualNodeKind::Main)
        .expect("the component compiles beside its continued style");
    (css, main)
}

#[test]
fn a_supplied_result_is_the_only_body_its_block_scopes() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let (canonical_id, request) = upsert(&host, None, SOURCE);
    admit(&host, &canonical_id, &request, PRODUCED);

    let (css, main) = compiled_with_supplied_result(&host, &canonical_id);
    assert!(
        css.contains(".card.svelte-") && css.contains("color: red"),
        "the produced bytes are the scoped body: {css}"
    );
    assert!(
        !css.contains("$tone"),
        "the authored block is never scoped in place of its result: {css}"
    );
    assert!(
        main.contains("card svelte-"),
        "the markup carries the scope class of the continued stylesheet: {main}"
    );
}

#[test]
fn fresh_and_edit_revert_compiles_of_a_continued_block_agree() {
    let fresh = VerterHost::new_standalone(HostConfig::default());
    let (fresh_id, fresh_request) = upsert(&fresh, None, SOURCE);
    admit(&fresh, &fresh_id, &fresh_request, PRODUCED);
    let expected = compiled_with_supplied_result(&fresh, &fresh_id);

    let host = VerterHost::new_standalone(HostConfig::default());
    let (canonical_id, request) = upsert(&host, None, SOURCE);
    admit(&host, &canonical_id, &request, PRODUCED);
    let _ = compiled_with_supplied_result(&host, &canonical_id);

    // The edit leaves the admitted result describing bytes the block no
    // longer holds: nothing may publish from it.
    let _ = upsert(&host, Some(canonical_id.clone()), EDITED);
    let stale = virtual_file(&host, &canonical_id, VirtualNodeKind::Style { index: 0 });
    assert!(
        stale
            .as_deref()
            .is_none_or(|css| !css.contains("color: red")),
        "a result for the pre-edit block must not publish after the edit: {stale:?}"
    );

    let (_, reverted_request) = upsert(&host, Some(canonical_id.clone()), SOURCE);
    admit(&host, &canonical_id, &reverted_request, PRODUCED);
    assert_eq!(
        compiled_with_supplied_result(&host, &canonical_id),
        expected,
        "the reverted, re-supplied block compiles exactly as a fresh one"
    );
}
