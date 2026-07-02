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
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("guard must read {}: {e}", path.display()));
    // Normalize line endings so the `\n`-anchored boundary searches below
    // (`fn_slice`, the top-level `"\n}\n"` slice) behave identically on CRLF
    // (Windows) and LF checkouts.
    source.replace("\r\n", "\n")
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

/// Every VIRTUAL-file provider branch runs the virtual post-await gate: the
/// captured surface must still be honored AND the virtual tab must still
/// byte-match the captured provider content before provider output is
/// mapped/returned.
#[test]
fn virtual_file_branches_validate_after_the_provider_await() {
    for (file, expected_gates) in [
        // hover + completion virtual branches.
        ("server/nav_features.rs", 2),
        // definition, type_definition, references virtual branches.
        ("server/nav_features_navigation.rs", 3),
        // document_highlight, signature_help, inlay_hint virtual branches.
        ("server/aux_features.rs", 3),
    ] {
        let source = read_server_source(file);
        let gates = source
            .matches(".virtual_request_surface_still_valid(")
            .count();
        assert!(
            gates >= expected_gates,
            "{file} must run the virtual post-await validation gate at least \
             {expected_gates} time(s) (found {gates})"
        );
    }
}

/// The BACKGROUND diagnostics publishers build their provider query from ONE
/// captured surface and re-validate after the await — never from independent
/// live reads with an unvalidated merge.
#[test]
fn background_diagnostics_paths_use_captured_surface_and_revalidate() {
    let sync_coordinator = read_server_source("sync_coordinator.rs");
    // The carrier + rune diagnostics helpers each capture and each revalidate.
    assert!(
        sync_coordinator
            .matches("capture_committed_carrier_ide_surface(")
            .count()
            >= 1,
        "the coordinator's carrier diagnostics must capture the committed CarrierIde surface"
    );
    assert!(
        sync_coordinator
            .matches("capture_committed_shadow_surface(")
            .count()
            >= 1,
        "the coordinator's rune diagnostics must capture the committed Shadow surface"
    );
    assert!(
        sync_coordinator
            .matches("captured_surface_still_valid_for_canonical(")
            .count()
            >= 2,
        "both background diagnostics helpers must re-validate the captured surface \
         after the provider await"
    );
    // The former torn live reads must not reappear inside the diagnostics
    // helpers: the IDE content/mapper come from the captured snapshot only.
    // (The SYNC path's own `get_ide` — delivering the surface, not querying
    // against it — is intentionally out of scope.)
    let top_level_fn_slice = |source: &str, fn_name: &str| -> String {
        let needle = format!("fn {fn_name}(");
        let start = source
            .find(&needle)
            .unwrap_or_else(|| panic!("guard must find `{needle}`"));
        let rest = &source[start..];
        let end = rest.find("\n}\n").map(|i| i + 2).unwrap_or(rest.len());
        rest[..end].to_string()
    };
    for helper in ["carrier_provider_diagnostics", "rune_module_diagnostics"] {
        let body = top_level_fn_slice(&sync_coordinator, helper);
        for forbidden in [
            "get_ide(",
            "PositionMapper::from_json",
            "get_position_mapper(",
        ] {
            assert!(
                !body.contains(forbidden),
                "{helper} must not rebuild the diagnostics context from the independent \
                 live read `{forbidden}` — the captured surface is the sole \
                 content/mapper authority"
            );
        }
    }

    // background_init routes BOTH its post-scan and post-init publishers through
    // the shared captured-surface helper and keeps no inline torn merge.
    let background_init = read_server_source("background_init.rs");
    assert!(
        background_init
            .matches("carrier_provider_diagnostics(")
            .count()
            >= 2,
        "background_init must route both diagnostics publishers through the shared \
         captured-surface helper"
    );
    for forbidden in [".get_diagnostics(", "merge_diagnostics("] {
        assert!(
            !background_init.contains(forbidden),
            "background_init.rs must not query/merge provider diagnostics inline \
             (`{forbidden}`) — the shared captured-surface helper owns that path"
        );
    }
}

/// FOREIGN carrier IDE locations map through the surface set pinned at request
/// start (`capture_foreign_carrier_ide_set` + `foreign_ide_context`); the
/// live-current foreign resolver is deleted and must not reappear.
#[test]
fn foreign_carrier_mapping_uses_request_start_pinned_set() {
    for file in [
        "server/nav_features_navigation.rs",
        "server/aux_features.rs",
        "server/child_prop_rename.rs",
        "server/sync_orchestration.rs",
        "server/provider_state.rs",
    ] {
        let source = read_server_source(file);
        assert!(
            !source.contains(".external_ide_context("),
            "{file} must not resolve a foreign carrier mapping through a live-current \
             `external_ide_context` — foreign locations map through the request-start \
             pinned set (`foreign_ide_context`)"
        );
    }
    // Every foreign-mapping consumer pins the set before the query.
    for (file, expected_pins) in [
        // definition, type_definition, references, rename.
        ("server/nav_features_navigation.rs", 4),
        // code actions.
        ("server/aux_features.rs", 1),
        // the imported-type declaration hop.
        ("server/child_prop_rename.rs", 1),
        // the push/pull diagnostics related-span resolver.
        ("server/sync_orchestration.rs", 1),
    ] {
        let source = read_server_source(file);
        let pins = source.matches("capture_foreign_carrier_ide_set(").count();
        assert!(
            pins >= expected_pins,
            "{file} must pin the foreign carrier IDE set before the provider query at \
             least {expected_pins} time(s) (found {pins})"
        );
    }
}
