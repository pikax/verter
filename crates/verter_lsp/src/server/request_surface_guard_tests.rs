//! Architecture guard: provider-backed interactive query contexts are built
//! from ONE captured immutable provider surface, never assembled from
//! independent live reads.
//!
//! The defect class this pins closed: a query context built from separate
//! reads of the committed provider path, the live-compiled IDE content, the
//! document projection mapper, and the document line index can be TORN by a
//! concurrent `did_change`/`did_close` interleaving between the reads — the
//! provider is then queried on an inconsistent tuple and its response mapped
//! through a mismatched mapper (wrong, not merely stale, results).

use std::path::Path;

fn read_server_source(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("guard must read {}: {e}", path.display()))
}

/// Extract the source slice of one `fn <name>` item: from its declaration to
/// the next `fn ` declaration at the same file (a coarse but stable slice —
/// these are long inherent methods separated by further methods).
fn fn_slice<'a>(source: &'a str, fn_name: &str) -> &'a str {
    let needle = format!("fn {fn_name}(");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("guard must find `{needle}`"));
    let rest = &source[start + needle.len()..];
    let end = rest.find("\n    pub(super) fn ").unwrap_or(rest.len());
    &rest[..end]
}

/// The shared interactive-query context builders resolve EVERYTHING through
/// the captured request surface: their bodies route through
/// `capture_provider_request_surface` and perform NONE of the independent
/// live reads the torn-tuple defect was assembled from.
#[test]
fn provider_query_context_builders_use_captured_surface_not_live_reads() {
    let sync_orchestration = read_server_source("server/sync_orchestration.rs");

    let builder = fn_slice(&sync_orchestration, "provider_projection_context");
    assert!(
        builder.contains("capture_provider_request_surface"),
        "provider_projection_context must build from the captured request surface"
    );
    for forbidden in [
        ".get_ide(",
        ".get_position_mapper(",
        ".active_ide_path_for_uri(",
        "self_file_provider_content",
    ] {
        assert!(
            !builder.contains(forbidden),
            "provider_projection_context must not assemble its context from the \
             independent live read `{forbidden}` — the captured surface is the sole \
             content/mapper authority"
        );
    }

    let provider_state = read_server_source("server/provider_state.rs");
    let virtual_ctx = fn_slice(&provider_state, "virtual_file_context");
    assert!(
        virtual_ctx.contains("capture_provider_request_surface"),
        "virtual_file_context must resolve the provider path through the captured \
         request surface"
    );
    assert!(
        !virtual_ctx.contains(".active_ide_path_for_uri("),
        "virtual_file_context must not resolve the provider path from an independent \
         committed-path read"
    );
}

/// Provider-backed feature handler modules never touch the live context
/// ingredients directly — they obtain a context through the snapshot-backed
/// builders (`type_provider_context` / `provider_projection_context` /
/// `virtual_file_context`) and re-validate it post-await.
#[test]
fn provider_backed_handlers_do_not_read_live_context_ingredients() {
    let handler_files = [
        "server/nav_features.rs",
        "server/nav_features_navigation.rs",
        "server/nav_features_completion_resolve.rs",
        "server/nav_features_hover_provenance.rs",
        "server/aux_features.rs",
        "server/child_prop_rename.rs",
        "server/component_resolve.rs",
    ];
    for file in handler_files {
        let source = read_server_source(file);
        for forbidden in [
            ".active_ide_path_for_uri(",
            ".get_ide(",
            ".get_position_mapper(",
        ] {
            assert!(
                !source.contains(forbidden),
                "{file} must not read the live context ingredient `{forbidden}` — \
                 provider-backed handlers go through the snapshot-backed context \
                 builders and the post-await validation gate"
            );
        }
    }
}

/// Every provider-backed handler that merges/maps a provider response back
/// onto the carrier runs the post-await validation gate.
#[test]
fn provider_backed_handlers_validate_after_the_provider_await() {
    for (file, expected_gates) in [
        // hover (main + redirect legs) + completion.
        ("server/nav_features.rs", 3),
        // definition, type_definition, references, rename.
        ("server/nav_features_navigation.rs", 4),
        // document_highlight, signature_help, code_action, semantic_tokens,
        // inlay_hint.
        ("server/aux_features.rs", 5),
        // getBindingTypes.
        ("server/custom_methods/mod.rs", 1),
        // The delegation inside `provider_context_still_valid` + the push/pull
        // diagnostics merge gate.
        ("server/sync_orchestration.rs", 2),
    ] {
        let source = read_server_source(file);
        // Dotted CALL sites only — `fn provider_context_still_valid(` and prose
        // mentions in doc comments carry no leading dot and are not counted.
        let gates = source.matches(".provider_context_still_valid(").count()
            + source
                .matches(".provider_request_surface_still_valid(")
                .count();
        assert!(
            gates >= expected_gates,
            "{file} must run the post-await validation gate at least {expected_gates} \
             time(s) (found {gates}) — a provider response produced against a \
             superseded surface must be dropped, never mapped"
        );
    }
}
