//! Same-file rename/references SYMBOL IDENTITY on the REAL LSP path.
//!
//! An SFC whose declarations live in a plain `<script>` (not `<script setup>`)
//! carries NO entries in `TemplateAnalysisSnapshot::binding_occurrences` — every
//! template occurrence is filed under `unresolved_bindings` instead, because the
//! compiler's template bindings map for a plain `<script>` is built from
//! `extract_options_bindings` (data/props/computed/methods/inject on the default
//! export) and never contains a top-level `const`.
//!
//! `unresolved_bindings` is NOT "the same symbol, filed elsewhere". The SAME map
//! that decides that classification decides the IDE accessor prefix: in-map ⇒
//! bare identifier, not-in-map ⇒ `___VERTER___instance.<name>`. So a span in
//! `unresolved_bindings` is an INSTANCE MEMBER — a different symbol from a
//! same-named module-level `const` in the script. Verter's file-local analysis
//! carries no signal separating "valid `ComponentCustomProperties` augmented
//! property" from "missing instance property", so the name-based native surface
//! must not answer at such a position at all: the TypeScript provider is the
//! sole semantic authority there, and an empty/absent provider answer means NO
//! edit (fail closed) rather than rewriting somebody else's symbol.
//!
//! These drive `<VerterLanguageServer as LanguageServer>::rename` /
//! `::references` / `::prepare_rename` — the production LSP entry points — over a
//! real `VerterHost`, with NO type provider (the native half IS the emitted
//! transaction) and with a mock provider (so the SERVER-level transaction, not
//! just the feature function, is what gets asserted). They assert EXACT edit
//! ranges, not counts.

use std::sync::Arc;

use tower_lsp_server::ls_types::{
    Position, PrepareRenameResponse, ReferenceContext, ReferenceParams, RenameParams,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Uri,
};
use tower_lsp_server::LanguageServer;
use verter_session::{HostConfig, VerterHost};

use super::super::VerterLanguageServer;

/// `count` is declared in a PLAIN `<script>` and spelled twice in the template —
/// the shape whose template occurrences land in `unresolved_bindings` and are
/// therefore instance-member accesses, not uses of the `const`.
const PLAIN_SCRIPT_SOURCE: &str = "<script lang=\"ts\">\nconst count = 1\nconsole.log(count)\n</script>\n\n<template>\n<div :title=\"count\">{{ count }}</div>\n</template>\n";

/// The SAME SFC with `<script setup>`: `count` IS in the template bindings map,
/// so the template spells the module-level binding itself and a rename must span
/// both regions.
const SCRIPT_SETUP_SOURCE: &str = "<script setup lang=\"ts\">\nconst count = 1\nconsole.log(count)\n</script>\n\n<template>\n<div :title=\"count\">{{ count }}</div>\n</template>\n";

/// The two LEXICAL script occurrences of `count` — the declaration and the
/// `console.log` usage — as `(line, start_character)`.
const SCRIPT_OCCURRENCES: [(u32, u32); 2] = [(1, 6), (2, 12)];

/// The two TEMPLATE spellings of `count`: the `:title` directive value and the
/// interpolation. In [`PLAIN_SCRIPT_SOURCE`] these are instance-member accesses
/// (`___VERTER___instance.count`), a DIFFERENT symbol from the script `const`.
const TEMPLATE_OCCURRENCES: [(u32, u32); 2] = [(6, 13), (6, 23)];

struct RenameFixture {
    _temp: tempfile::TempDir,
    service: tower_lsp_server::LspService<VerterLanguageServer>,
    drain: tokio::task::JoinHandle<()>,
    uri: Uri,
}

impl RenameFixture {
    fn server(&self) -> &VerterLanguageServer {
        self.service.inner()
    }

    async fn shutdown(self) {
        self.drain.abort();
        drop(self.service);
    }
}

/// A fixture with NO type provider: the native half IS the emitted transaction,
/// so whatever the native half claims ships verbatim to the editor — and where
/// the native half must not answer, the answer is nothing.
async fn fixture(source: &str) -> RenameFixture {
    fixture_with_provider(source, None).await
}

async fn fixture_with_provider(
    source: &str,
    type_provider: Option<Arc<dyn crate::TypeProvider>>,
) -> RenameFixture {
    fixture_with_provider_kind(source, type_provider, crate::TypeProviderKind::Tsgo).await
}

async fn fixture_with_provider_kind(
    source: &str,
    type_provider: Option<Arc<dyn crate::TypeProvider>>,
    kind: crate::TypeProviderKind,
) -> RenameFixture {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("workspace dir");
    std::fs::write(workspace.join("tsconfig.json"), "{}").expect("write tsconfig");
    std::fs::write(workspace.join("src/App.vue"), source).expect("write sfc");

    let vfs_access: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_access));
    let host_for_server = Arc::clone(&host);
    let (service, socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            crate::LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: type_provider.clone(),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: kind,
                type_provider_topology: crate::TypeProviderTopology::implied_by(kind),
                mcp_port: None,
                type_provider_reason: None,
                type_provider_advisory: None,
                suppress_imported_carrier_prewarm: false,
            },
        )
    });
    let drain = tokio::spawn(async move {
        let mut socket = socket;
        while futures_util::StreamExt::next(&mut socket).await.is_some() {}
    });

    let workspace_id = crate::test_utils::canonical_test_path(&workspace);
    let server = service.inner();
    server.documents.set_semantic_analysis_enabled(true);
    host.configure_projects(vec![crate::project_resolver::IdeProjectConfig::new(
        workspace_id.clone(),
        workspace_id.clone(),
        Some(format!("{workspace_id}/tsconfig.json")),
    )]);

    let canonical_id = format!("{workspace_id}/src/App.vue");
    let uri = crate::uri::path_to_file_uri(&canonical_id).expect("file uri");
    let mut semantic_ready = server.documents.subscribe_semantic_ready();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });
    server.documents.schedule_semantic_analysis(&uri);
    tokio::time::timeout(std::time::Duration::from_secs(20), semantic_ready.recv())
        .await
        .expect("semantic analysis must settle")
        .expect("semantic ready channel stays open");

    RenameFixture {
        _temp: temp,
        service,
        drain,
        uri,
    }
}

fn position_of(source: &str, needle: &str, delta: usize) -> Position {
    let offset = source
        .find(needle)
        .unwrap_or_else(|| panic!("needle `{needle}` should exist"))
        + delta;
    let before = &source[..offset];
    Position {
        line: before.matches('\n').count() as u32,
        character: (offset - before.rfind('\n').map_or(0, |i| i + 1)) as u32,
    }
}

/// Every `(line, character)` start the emitted `WorkspaceEdit` writes, sorted.
fn edit_starts(edit: &tower_lsp_server::ls_types::WorkspaceEdit, uri: &Uri) -> Vec<(u32, u32)> {
    let mut starts: Vec<(u32, u32)> = edit
        .changes
        .as_ref()
        .and_then(|changes| changes.get(uri))
        .map(|edits| {
            edits
                .iter()
                .map(|e| (e.range.start.line, e.range.start.character))
                .collect()
        })
        .unwrap_or_default();
    starts.sort_unstable();
    starts
}

async fn rename_at(
    server: &VerterLanguageServer,
    uri: &Uri,
    position: Position,
    new_name: &str,
) -> Option<tower_lsp_server::ls_types::WorkspaceEdit> {
    server
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            new_name: new_name.to_string(),
            work_done_progress_params: Default::default(),
        })
        .await
        .expect("rename request succeeds")
}

async fn references_at(
    server: &VerterLanguageServer,
    uri: &Uri,
    position: Position,
) -> Option<Vec<tower_lsp_server::ls_types::Location>> {
    server
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .expect("references request succeeds")
}

fn location_starts(locations: &[tower_lsp_server::ls_types::Location]) -> Vec<(u32, u32)> {
    let mut starts: Vec<(u32, u32)> = locations
        .iter()
        .map(|l| (l.range.start.line, l.range.start.character))
        .collect();
    starts.sort_unstable();
    starts
}

async fn prepare_rename_at(
    server: &VerterLanguageServer,
    uri: &Uri,
    position: Position,
) -> Option<PrepareRenameResponse> {
    server
        .prepare_rename(TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        })
        .await
        .expect("prepare_rename request succeeds")
}

/// The script declaration anchor renames the module-level `const` — which in a
/// plain-`<script>` SFC means EXACTLY its two lexical script occurrences. The
/// template spellings are instance-member accesses on a different symbol; an
/// edit there would rewrite whatever `ComponentCustomProperties` augmentation
/// (or Options-API surface) actually owns that name.
#[tokio::test]
async fn script_anchor_renames_exactly_the_two_lexical_script_occurrences() {
    let f = fixture(PLAIN_SCRIPT_SOURCE).await;
    let position = position_of(PLAIN_SCRIPT_SOURCE, "const count", "const co".len());

    let edit = rename_at(f.server(), &f.uri, position, "renamed")
        .await
        .expect("a script binding under the cursor renames");
    let starts = edit_starts(&edit, &f.uri);

    assert_eq!(
        starts,
        SCRIPT_OCCURRENCES.to_vec(),
        "the module-level `const` owns only its script occurrences"
    );
    for template_start in TEMPLATE_OCCURRENCES {
        assert!(
            !starts.contains(&template_start),
            "the template spelling at {template_start:?} is an instance-member access — a \
             DIFFERENT symbol — and must not be rewritten by a script-declaration rename"
        );
    }
    let changes = edit.changes.as_ref().expect("changes map");
    assert_eq!(
        changes.len(),
        1,
        "a same-file script rename touches exactly one file — no augmentation file edit"
    );
    let edits = changes.get(&f.uri).expect("edits for the renamed file");
    for e in edits {
        assert_eq!(e.new_text, "renamed", "every edit writes the new name");
        assert_eq!(
            e.range.end,
            Position {
                line: e.range.start.line,
                character: e.range.start.character + 5,
            },
            "each edit spans exactly the 5-character identifier `count`, at {:?}",
            e.range
        );
    }
    f.shutdown().await;
}

/// A cursor INSIDE an instance-member template access is the provider's symbol,
/// not Verter's. With no provider there is no semantic authority to answer, so
/// the rename fails closed — and above all it never rewrites the module `const`.
#[tokio::test]
async fn template_instance_member_anchor_fails_closed_without_a_provider() {
    let f = fixture(PLAIN_SCRIPT_SOURCE).await;
    let position = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());

    let edit = rename_at(f.server(), &f.uri, position, "renamed").await;

    assert!(
        edit.is_none(),
        "an instance-member anchor with no provider answer must emit NO WorkspaceEdit, got {edit:?}"
    );
    f.shutdown().await;
}

/// The same rule at the OTHER instance-member spelling — the `:title` directive
/// value — so the refusal is positional, not interpolation-specific.
#[tokio::test]
async fn directive_value_instance_member_anchor_fails_closed_without_a_provider() {
    let f = fixture(PLAIN_SCRIPT_SOURCE).await;
    let position = position_of(PLAIN_SCRIPT_SOURCE, ":title=\"count\"", ":title=\"co".len());

    let edit = rename_at(f.server(), &f.uri, position, "renamed").await;

    assert!(
        edit.is_none(),
        "a `:title=\"count\"` instance-member anchor must emit NO WorkspaceEdit, got {edit:?}"
    );
    f.shutdown().await;
}

/// `prepare_rename` must not advertise the instance-member position as
/// renameable by the native name-based surface — it would hand the editor the
/// module `const`'s word range for a symbol Verter cannot resolve. The script
/// anchor stays renameable.
#[tokio::test]
async fn prepare_rename_refuses_the_instance_member_anchor_but_allows_the_script_anchor() {
    let f = fixture(PLAIN_SCRIPT_SOURCE).await;

    let template_position = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());
    let template_prepare = prepare_rename_at(f.server(), &f.uri, template_position).await;
    assert!(
        template_prepare.is_none(),
        "the instance-member anchor is not natively renameable, got {template_prepare:?}"
    );

    let script_position = position_of(PLAIN_SCRIPT_SOURCE, "const count", "const co".len());
    let script_prepare = prepare_rename_at(f.server(), &f.uri, script_position).await;
    match script_prepare {
        Some(PrepareRenameResponse::Range(range)) => {
            assert_eq!(
                (range.start.line, range.start.character),
                SCRIPT_OCCURRENCES[0],
                "the script declaration's own word range is offered"
            );
        }
        other => panic!("the script declaration must stay renameable, got {other:?}"),
    }
    f.shutdown().await;
}

/// `references` answers the SAME symbol question, so it stops at the script
/// occurrences too. Reporting the instance-member spellings would tell the user
/// the `const` is used in the template, which is exactly the belief that made
/// the rename rewrite them.
#[tokio::test]
async fn references_from_the_script_declaration_stop_at_the_script_occurrences() {
    let f = fixture(PLAIN_SCRIPT_SOURCE).await;
    let position = position_of(PLAIN_SCRIPT_SOURCE, "const count", "const co".len());

    let locations = references_at(f.server(), &f.uri, position)
        .await
        .expect("the script binding under the cursor has references");
    let starts = location_starts(&locations);

    assert_eq!(
        starts,
        SCRIPT_OCCURRENCES.to_vec(),
        "references to the module-level `const` are its script occurrences"
    );
    for template_start in TEMPLATE_OCCURRENCES {
        assert!(
            !starts.contains(&template_start),
            "the instance-member spelling at {template_start:?} is not a reference to the \
             script `const`"
        );
    }
    f.shutdown().await;
}

/// From the instance-member anchor the native name-based surface is suppressed
/// entirely — it must not answer with the script `const`'s occurrences.
#[tokio::test]
async fn references_at_an_instance_member_anchor_do_not_report_the_script_const() {
    let f = fixture(PLAIN_SCRIPT_SOURCE).await;
    let position = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());

    let locations = references_at(f.server(), &f.uri, position).await;

    let starts = locations
        .as_deref()
        .map(location_starts)
        .unwrap_or_default();
    for script_start in SCRIPT_OCCURRENCES {
        assert!(
            !starts.contains(&script_start),
            "an instance-member anchor must not report the script `const` occurrence at \
             {script_start:?}; got {starts:?}"
        );
    }
    assert!(
        locations.is_none(),
        "with no provider there is no authority for the instance member — expected no \
         references, got {starts:?}"
    );
    f.shutdown().await;
}

/// The CONTRAST case, and the one the union model was right about: with
/// `<script setup>` the template genuinely spells the module-level binding
/// (it IS in the template bindings map, so it lowers to a bare identifier), and
/// the cross-region rename must keep spanning script AND template.
#[tokio::test]
async fn script_setup_rename_still_spans_script_and_template() {
    let f = fixture(SCRIPT_SETUP_SOURCE).await;
    let position = position_of(SCRIPT_SETUP_SOURCE, "const count", "const co".len());

    let edit = rename_at(f.server(), &f.uri, position, "renamed")
        .await
        .expect("a `<script setup>` binding under the cursor renames");
    let starts = edit_starts(&edit, &f.uri);

    let mut expected: Vec<(u32, u32)> = SCRIPT_OCCURRENCES.to_vec();
    expected.extend(TEMPLATE_OCCURRENCES);
    expected.sort_unstable();
    assert_eq!(
        starts, expected,
        "a `<script setup>` binding is one symbol across script and template — the rename \
         spans both regions"
    );
    f.shutdown().await;
}

/// The template-anchored rename works from the template side too, when the
/// template really does spell the script binding.
#[tokio::test]
async fn script_setup_rename_from_the_template_spans_both_regions() {
    let f = fixture(SCRIPT_SETUP_SOURCE).await;
    let position = position_of(SCRIPT_SETUP_SOURCE, "{{ count }}", "{{ co".len());

    let edit = rename_at(f.server(), &f.uri, position, "renamed")
        .await
        .expect("a `<script setup>` binding under the template cursor renames");
    let starts = edit_starts(&edit, &f.uri);

    let mut expected: Vec<(u32, u32)> = SCRIPT_OCCURRENCES.to_vec();
    expected.extend(TEMPLATE_OCCURRENCES);
    expected.sort_unstable();
    assert_eq!(
        starts, expected,
        "the interpolation under the cursor and the script declaration are one symbol"
    );
    f.shutdown().await;
}

/// Plain markup TEXT that merely spells the binding name is NOT an occurrence,
/// and in a plain-`<script>` SFC neither is the interpolation: the declaration
/// alone is rewritten.
#[tokio::test]
async fn plain_template_text_matching_the_name_is_not_renamed() {
    const PROSE: &str = "<script lang=\"ts\">\nconst count = 1\n</script>\n\n<template>\n<div>count: {{ count }}</div>\n</template>\n";
    let f = fixture(PROSE).await;
    let position = position_of(PROSE, "const count", "const co".len());

    let edit = rename_at(f.server(), &f.uri, position, "renamed")
        .await
        .expect("a binding under the cursor renames");
    let starts = edit_starts(&edit, &f.uri);

    assert_eq!(
        starts,
        vec![(1, 6)],
        "only the declaration is edited — the `count:` prose is not an occurrence, and the \
         interpolation is an instance-member access on a different symbol"
    );
    f.shutdown().await;
}

/// A `v-for` iteration variable SHADOWING the script binding is a different
/// symbol. Its occurrences must NOT be renamed even though they spell the same
/// name — template expression capture already excludes locals.
#[tokio::test]
async fn a_v_for_variable_shadowing_the_binding_is_not_renamed() {
    const SHADOWED: &str = "<script lang=\"ts\">\nconst count = 1\nconst list = [1, 2]\n</script>\n\n<template>\n<div v-for=\"count in list\">{{ count }}</div>\n</template>\n";
    let f = fixture(SHADOWED).await;
    let position = position_of(SHADOWED, "const count", "const co".len());

    let edit = rename_at(f.server(), &f.uri, position, "renamed")
        .await
        .expect("a binding under the cursor renames");
    let starts = edit_starts(&edit, &f.uri);

    assert_eq!(
        starts,
        vec![(1, 6)],
        "only the shadowed declaration is renamed — the `v-for` local and its use belong to \
         a different scope"
    );
    f.shutdown().await;
}

// ── The emitted SERVER transaction, not just the feature function ───────────
//
// `same_file_rename_ranges` answering `None` is not the same fact as the server
// emitting no `WorkspaceEdit`: `handle_rename` merges a provider location set
// into the native half and can mint a `WorkspaceEdit` of its own. These drive a
// mock provider so the assertion is over the transaction the editor receives.
//
// `TypeProviderKind::None` alongside an injected provider is the documented
// embedder route (`prepare_workspace_symbol_frontier`: "Embedders/tests may
// inject a provider without the managed store topology … delegate completeness
// to that provider"), which is what lets these reach the provider query at all.

/// The `{carrier}.tsx` path and byte offset `handle_rename` queries the provider
/// with for `position` — computed exactly as the handler computes it, so a mock
/// response keyed on it is the response the handler actually receives.
async fn provider_query_anchor(
    server: &VerterLanguageServer,
    uri: &Uri,
    position: Position,
) -> (String, u32) {
    server.ensure_current_file_synced(uri).await;
    let ctx = server
        .repaired_type_provider_context(uri)
        .await
        .expect("the carrier has a provider context");
    let offset = crate::type_provider::merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("the template anchor maps into the generated TSX");
    (ctx.tsx_path.clone(), offset)
}

/// A provider that answers the instance-member anchor owns that symbol, and its
/// edits are the WHOLE transaction: the module-level `const` is a different
/// symbol and must not be swept in.
#[tokio::test]
async fn instance_member_anchor_edit_is_provider_owned_and_never_the_script_const() {
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_with_provider_kind(
        PLAIN_SCRIPT_SOURCE,
        Some(provider.clone()),
        crate::TypeProviderKind::None,
    )
    .await;
    let position = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());
    let (tsx_path, query_offset) = provider_query_anchor(f.server(), &f.uri, position).await;
    // The location the provider reports is the WHOLE token, which starts two
    // bytes before the cursor.
    let token_start = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ ".len());
    let (_, token_offset) = provider_query_anchor(f.server(), &f.uri, token_start).await;

    // The provider resolves the instance property and reports its own
    // occurrence — the `count` token of `___VERTER___instance.count` in the
    // generated TSX, which maps back onto the interpolation.
    provider.set_rename_locations(
        &tsx_path,
        query_offset,
        vec![crate::type_provider::protocol::RenameLocation {
            path: tsx_path.clone(),
            start: token_offset,
            end: token_offset + "count".len() as u32,
        }],
    );

    let edit = rename_at(f.server(), &f.uri, position, "renamed")
        .await
        .expect("a provider that resolves the instance member renames it");
    let starts = edit_starts(&edit, &f.uri);

    assert_eq!(
        starts,
        vec![TEMPLATE_OCCURRENCES[1]],
        "the emitted transaction is exactly the provider's own occurrence"
    );
    for script_start in SCRIPT_OCCURRENCES {
        assert!(
            !starts.contains(&script_start),
            "the module-level `const` at {script_start:?} is a DIFFERENT symbol and must never \
             be swept into an instance-member rename; got {starts:?}"
        );
    }
    f.shutdown().await;
}

/// A provider that answers EMPTY has not resolved the instance member, so there
/// is no authority for the position and NO `WorkspaceEdit` ships — not an empty
/// one, and above all not the script `const`'s edits. The provider IS consulted:
/// this is a completeness refusal, not a short-circuit that never asked.
#[tokio::test]
async fn instance_member_anchor_consults_the_provider_and_ships_nothing_when_it_answers_empty() {
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_with_provider_kind(
        PLAIN_SCRIPT_SOURCE,
        Some(provider.clone()),
        crate::TypeProviderKind::None,
    )
    .await;
    let position = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());

    // No configured response: `MockTypeProvider::get_rename_locations` answers
    // `Ok(vec![])`, which is also what a provider denied by feature admission
    // serves.
    let edit = rename_at(f.server(), &f.uri, position, "renamed").await;

    assert!(
        edit.is_none(),
        "an empty provider answer must ship NO WorkspaceEdit at all, got {edit:?}"
    );
    assert!(
        provider.calls().iter().any(|call| matches!(
            call,
            crate::type_provider::mock::MockCall::GetRenameLocations { .. }
        )),
        "the provider owns this position, so it must actually be asked; calls: {:?}",
        provider.calls()
    );
    f.shutdown().await;
}

/// The suppression is POSITIONAL and half-open: every offset from the token's
/// FIRST byte through its LAST is inside the instance-member span, and one past
/// the token is outside every identifier so nothing resolves there either.
/// Neither end may leak into the name-based branch and rewrite the script
/// `const`. The `<script setup>` contrast proves this is not a blanket refusal:
/// the same offsets rename across both regions when the template really does
/// spell the module binding.
#[tokio::test]
async fn instance_member_suppression_spans_the_whole_token_and_stops_at_its_end() {
    let f = fixture(PLAIN_SCRIPT_SOURCE).await;

    for (label, delta) in [
        ("first byte", "{{ ".len()),
        ("interior", "{{ coun".len()),
        ("last byte", "{{ coun".len()),
        ("one past the last byte", "{{ count".len()),
    ] {
        let position = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", delta);
        let edit = rename_at(f.server(), &f.uri, position, "renamed").await;
        assert!(
            edit.is_none(),
            "{label}: an instance-member offset must not rename the script `const`, got {edit:?}"
        );
    }
    f.shutdown().await;

    let setup = fixture(SCRIPT_SETUP_SOURCE).await;
    let mut expected: Vec<(u32, u32)> = SCRIPT_OCCURRENCES.to_vec();
    expected.extend(TEMPLATE_OCCURRENCES);
    expected.sort_unstable();
    for (label, delta) in [("first byte", "{{ ".len()), ("last byte", "{{ coun".len())] {
        let position = position_of(SCRIPT_SETUP_SOURCE, "{{ count }}", delta);
        let edit = rename_at(setup.server(), &setup.uri, position, "renamed")
            .await
            .unwrap_or_else(|| panic!("{label}: a `<script setup>` binding stays renameable"));
        assert_eq!(
            edit_starts(&edit, &setup.uri),
            expected,
            "{label}: the same offsets still span script and template for a real binding"
        );
    }
    setup.shutdown().await;
}
