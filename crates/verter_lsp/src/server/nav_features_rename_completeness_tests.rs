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

/// A TYPE-ONLY import binding: excluded from the native rename surface
/// (`classify_rename_target` skips `is_type_only` imports and their type-only
/// bindings) and owned by no CSS name, so the cursor resolves to
/// `RenameTargetClass::Unavailable` while the TypeScript provider legitimately
/// owns renaming the type across files.
const TYPE_ONLY_IMPORT_SOURCE: &str = "<script setup lang=\"ts\">\nimport type { Widget } from \"./widget\"\nconst w = null as unknown as Widget\n</script>\n\n<template>\n<div>{{ w }}</div>\n</template>\n";

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
    fixture_for_carrier(source, type_provider, kind, "App.vue", "vue").await
}

/// The same fixture over an arbitrary CARRIER file name + editor language id, so
/// a lane can drive the real LSP rename path on a `.svelte` carrier as well as a
/// `.vue` one.
async fn fixture_for_carrier(
    source: &str,
    type_provider: Option<Arc<dyn crate::TypeProvider>>,
    kind: crate::TypeProviderKind,
    file_name: &str,
    language_id: &str,
) -> RenameFixture {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("workspace dir");
    std::fs::write(workspace.join("tsconfig.json"), "{}").expect("write tsconfig");
    std::fs::write(workspace.join("src").join(file_name), source).expect("write carrier");

    let vfs_workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let vfs_access: Arc<dyn verter_workspace::WorkspaceAccess> = vfs_workspace.clone();
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
    let configured = vfs_workspace
        .load_published()
        .expect("configure_projects must publish the complete test graph");
    let snapshot = Arc::clone(&configured.snapshot);
    let views = crate::workspace_state::build_lsp_views(&*vfs_workspace, &snapshot, vec![]);
    vfs_workspace.publish_snapshot(verter_workspace::PublishedRoot::with_ext(
        snapshot,
        Box::new(views),
    ));
    server.install_vfs_workspace(vfs_workspace);

    let canonical_id = format!("{workspace_id}/src/{file_name}");
    let uri = crate::uri::path_to_file_uri(&canonical_id).expect("file uri");
    let mut semantic_ready = server.documents.subscribe_semantic_ready();
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: language_id.to_string(),
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

fn rename_query_count(provider: &crate::type_provider::mock::MockTypeProvider) -> usize {
    provider
        .calls()
        .iter()
        .filter(|call| {
            matches!(
                call,
                crate::type_provider::mock::MockCall::GetRenameLocations { .. }
            )
        })
        .count()
}

fn assert_public_prop_refusal(error: &tower_lsp_server::jsonrpc::Error) {
    assert_eq!(
        error.code,
        tower_lsp_server::jsonrpc::ErrorCode::ServerError(-32803),
        "public component-prop refusal must use LSP RequestFailed"
    );
    assert!(
        error.message.contains("complete cross-file usage proof"),
        "the error must explain the missing completeness proof, got {:?}",
        error.message
    );
    assert!(
        error.message.contains("no rename edit was produced"),
        "the error must make the no-edit outcome explicit, got {:?}",
        error.message
    );
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

/// A child-side Vue public prop is not renameable until the workspace can prove
/// every parent usage has been enumerated. Prepare must not advertise it, and a
/// direct request must fail before the provider rename authority is queried.
#[tokio::test]
async fn vue_define_props_member_refuses_before_provider_rename() {
    const SOURCE: &str =
        "<script setup lang=\"ts\">\ndefineProps<{ title: string }>()\n</script>\n";
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_with_provider(SOURCE, Some(provider.clone())).await;
    let position = position_of(SOURCE, "title: string", 2);
    let before = rename_query_count(&provider);

    let prepared = prepare_rename_at(f.server(), &f.uri, position).await;
    assert!(
        prepared.is_none(),
        "prepare must not offer a public component-prop rename"
    );
    assert_eq!(
        rename_query_count(&provider),
        before,
        "prepare must classify the public prop without probing provider rename"
    );

    let result = rename_result_at(f.server(), &f.uri, position, "heading").await;
    let error = result.expect_err(
        "a direct public component-prop rename must return a clear error, never a WorkspaceEdit",
    );
    assert_public_prop_refusal(&error);
    assert_eq!(
        rename_query_count(&provider),
        before,
        "direct rename must refuse before calling the provider"
    );
    f.shutdown().await;
}

/// The Svelte public-API projector maps local `$props()` member names exactly
/// back to the authored declaration. That public key must not fall through the
/// `NotChildProp` / provider-only path merely because Vue macro analysis does
/// not classify it.
#[tokio::test]
async fn svelte_props_member_refuses_without_provider_passthrough() {
    const SOURCE: &str =
        "<script lang=\"ts\">\nlet { title }: { title: string } = $props();\n</script>\n";
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_for_carrier(
        SOURCE,
        Some(provider.clone()),
        crate::TypeProviderKind::Tsgo,
        "App.svelte",
        "svelte",
    )
    .await;
    let position = position_of(SOURCE, "{ title: string", 3);
    let before = rename_query_count(&provider);

    let prepared = prepare_rename_at(f.server(), &f.uri, position).await;
    assert!(
        prepared.is_none(),
        "prepare must not offer a Svelte public prop rename"
    );
    assert_eq!(
        rename_query_count(&provider),
        before,
        "prepare must not probe provider rename for a Svelte public prop"
    );

    let result = rename_result_at(f.server(), &f.uri, position, "heading").await;
    let error = result.expect_err(
        "a Svelte public prop must fail closed instead of reaching provider passthrough",
    );
    assert_public_prop_refusal(&error);
    assert_eq!(
        rename_query_count(&provider),
        before,
        "the Svelte public prop must be refused before provider rename"
    );
    f.shutdown().await;
}

/// KNOWN OPEN: untyped `$props()` destructured keys are not classified as public
/// props. This is unsound pending a producer-owned destructured-key span
/// inventory: the name-only fallback treats the shorthand key as an ordinary
/// binding and can emit a same-file edit without proving parent usages.
#[tokio::test]
async fn known_open_svelte_untyped_props_key_is_not_refused_without_producer_inventory() {
    const SOURCE: &str =
        "<script lang=\"ts\">\nlet { title } = $props();\nconsole.log(title)\n</script>\n";
    let f = fixture_for_carrier(
        SOURCE,
        None,
        crate::TypeProviderKind::Tsgo,
        "Untyped.svelte",
        "svelte",
    )
    .await;
    let position = position_of(SOURCE, "{ title }", 3);

    assert!(
        prepare_rename_at(f.server(), &f.uri, position)
            .await
            .is_some(),
        "the known-open untyped key currently follows ordinary binding behavior"
    );
    let edit = rename_result_at(f.server(), &f.uri, position, "heading")
        .await
        .expect("the known-open untyped key is not categorically refused")
        .expect("the current name-only fallback emits its same-file binding edit");
    assert_eq!(
        edit_starts(&edit, &f.uri),
        vec![(1, 6), (2, 12)],
        "current honest behavior renames only the key/local binding spellings"
    );
    f.shutdown().await;
}

/// KNOWN OPEN: the untyped `$props()` construct has no blanket refusal because
/// the available facts cannot distinguish public destructured keys from local
/// bindings. This local alias therefore follows ordinary binding behavior. The
/// unsound public-key side stays open until a producer-owned destructured-key
/// span inventory exists.
#[tokio::test]
async fn known_open_svelte_props_local_alias_is_not_refused_without_producer_inventory() {
    const SOURCE: &str =
        "<script lang=\"ts\">\nlet { title: localTitle } = $props();\nconsole.log(localTitle)\n</script>\n";
    let f = fixture_for_carrier(
        SOURCE,
        None,
        crate::TypeProviderKind::Tsgo,
        "Alias.svelte",
        "svelte",
    )
    .await;
    let position = position_of(SOURCE, "localTitle", 3);

    assert!(
        prepare_rename_at(f.server(), &f.uri, position)
            .await
            .is_some(),
        "the local binding is not categorically refused with the untyped construct"
    );

    let edit = rename_result_at(f.server(), &f.uri, position, "heading")
        .await
        .expect("the local alias inside untyped `$props()` destructuring is not refused")
        .expect("the ordinary binding path emits a same-file edit");
    assert_eq!(
        edit_starts(&edit, &f.uri),
        vec![(1, 13), (2, 12)],
        "only the local alias declaration and use are renamed"
    );
    f.shutdown().await;
}

/// The finding-1 shape: a public key preceding its aliased leaf has no
/// producer-owned key-span anchor. With a same-named import, the name-only
/// fallback currently mistakes the public key for that import and emits a
/// native edit. This deliberately pins the honest known-open behavior so a
/// future producer-owned destructured-key inventory has a discriminating case.
#[tokio::test]
async fn known_open_svelte_props_key_before_alias_can_follow_same_named_import() {
    const SOURCE: &str = "<script lang=\"ts\">\nimport { title } from \"./constants\";\nlet { title: localTitle } = $props();\nconsole.log(localTitle);\n</script>\n";
    let f = fixture_for_carrier(
        SOURCE,
        None,
        crate::TypeProviderKind::Tsgo,
        "ImportedAlias.svelte",
        "svelte",
    )
    .await;
    let position = position_of(SOURCE, "let { title", "let { ".len());

    assert!(
        prepare_rename_at(f.server(), &f.uri, position)
            .await
            .is_some(),
        "the known-open key is currently mistaken for the same-named import"
    );
    let edit = rename_result_at(f.server(), &f.uri, position, "heading")
        .await
        .expect("the known-open public key is not categorically refused")
        .expect("the name-only import fallback emits a native WorkspaceEdit");
    assert_eq!(
        edit_starts(&edit, &f.uri),
        vec![(1, 9), (2, 6)],
        "current behavior renames the unrelated import and public key, but not the local alias"
    );
    f.shutdown().await;
}

/// Unrelated script bindings elsewhere in a Svelte `$props()` file remain
/// ordinary local renames.
#[tokio::test]
async fn ordinary_svelte_binding_elsewhere_in_props_file_still_renames() {
    const SOURCE: &str = "<script lang=\"ts\">\nconst beforeValue = 1;\nlet { title } = $props();\nconst afterValue = 2;\nconsole.log(beforeValue, afterValue);\n</script>\n";
    let f = fixture_for_carrier(
        SOURCE,
        None,
        crate::TypeProviderKind::Tsgo,
        "Mixed.svelte",
        "svelte",
    )
    .await;
    for (needle, delta, new_name, expected) in [
        (
            "const beforeValue",
            "const before".len(),
            "renamedBefore",
            vec![(1, 6), (4, 12)],
        ),
        (
            "const afterValue",
            "const after".len(),
            "renamedAfter",
            vec![(3, 6), (4, 25)],
        ),
    ] {
        let position = position_of(SOURCE, needle, delta);
        let edit = rename_result_at(f.server(), &f.uri, position, new_name)
            .await
            .expect("ordinary script rename request succeeds")
            .expect("an unrelated binding in a `$props()` file stays renameable");
        assert_eq!(
            edit_starts(&edit, &f.uri),
            expected,
            "only the unrelated binding declaration and usage are renamed"
        );
    }
    f.shutdown().await;
}

#[tokio::test]
async fn vue_runtime_options_and_svelte_legacy_prop_declarations_refuse() {
    for (source, needle, delta, file_name, language_id) in [
        (
            "<script setup lang=\"ts\">\ndefineProps({ title: String })\n</script>\n",
            "title: String",
            2,
            "Runtime.vue",
            "vue",
        ),
        (
            "<script lang=\"ts\">\nexport default { props: { title: String } }\n</script>\n",
            "title: String",
            2,
            "Options.vue",
            "vue",
        ),
        (
            "<script lang=\"ts\">\nexport let title: string;\n</script>\n",
            "title: string",
            2,
            "Legacy.svelte",
            "svelte",
        ),
    ] {
        let f = fixture_for_carrier(
            source,
            None,
            crate::TypeProviderKind::Tsgo,
            file_name,
            language_id,
        )
        .await;
        let position = position_of(source, needle, delta);
        assert!(
            prepare_rename_at(f.server(), &f.uri, position)
                .await
                .is_none(),
            "{file_name}: prepare must not offer a public prop"
        );
        let error = rename_result_at(f.server(), &f.uri, position, "heading")
            .await
            .expect_err("public prop declaration must return no WorkspaceEdit");
        assert_public_prop_refusal(&error);
        f.shutdown().await;
    }
}

/// A component usage remains prop-shaped even when child resolution fails.
/// This is the exact `NotChildProp` fallthrough hole: absence of a resolved
/// child cannot authorize provider passthrough.
#[tokio::test]
async fn unresolved_component_prop_usage_refuses_before_provider_rename() {
    for (source, file_name, language_id) in [
        (
            "<script setup lang=\"ts\">\nconst value = 'x'\n</script>\n<template><MissingChild :title=\"value\" /></template>\n",
            "Parent.vue",
            "vue",
        ),
        (
            "<script lang=\"ts\">\nconst value = 'x';\n</script>\n<MissingChild title={value} />\n",
            "Parent.svelte",
            "svelte",
        ),
    ] {
        let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
        let f = fixture_for_carrier(
            source,
            Some(provider.clone()),
            crate::TypeProviderKind::Tsgo,
            file_name,
            language_id,
        )
        .await;
        let position = position_of(source, "title", 2);
        let before = rename_query_count(&provider);
        let error = rename_result_at(f.server(), &f.uri, position, "heading")
            .await
            .expect_err("an unresolved prop-shaped cursor must fail closed");
        assert_public_prop_refusal(&error);
        assert_eq!(
            rename_query_count(&provider),
            before,
            "{file_name}: unresolved child must not reach provider rename"
        );
        f.shutdown().await;
    }
}

/// Ordinary script bindings remain renameable in both carrier frameworks. This
/// is the negative control for the public-prop-only refusal.
#[tokio::test]
async fn ordinary_vue_and_svelte_script_bindings_still_produce_workspace_edits() {
    for (source, file_name, language_id) in [
        (
            "<script setup lang=\"ts\">\nconst localValue = 1\nconsole.log(localValue)\n</script>\n",
            "App.vue",
            "vue",
        ),
        (
            "<script lang=\"ts\">\nconst localValue = 1;\nconsole.log(localValue);\n</script>\n",
            "App.svelte",
            "svelte",
        ),
    ] {
        let f = fixture_for_carrier(
            source,
            None,
            crate::TypeProviderKind::Tsgo,
            file_name,
            language_id,
        )
        .await;
        let position = position_of(source, "const localValue", "const local".len());
        let edit = rename_result_at(f.server(), &f.uri, position, "renamedValue")
            .await
            .expect("ordinary script rename request succeeds")
            .expect("ordinary script binding still produces a WorkspaceEdit");
        assert_eq!(
            edit_starts(&edit, &f.uri),
            vec![(1, 6), (2, 12)],
            "{file_name}: only the declaration and script use are renamed"
        );
        f.shutdown().await;
    }
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

/// PROVIDER-ABSENT lane of the prepare matrix. `prepare_rename` must not
/// advertise the instance-member position as renameable by the native name-based
/// surface — it would hand the editor the module `const`'s word range for a
/// symbol Verter cannot resolve — and with NO provider configured there is no
/// authority to consult, so the answer is nothing. The script anchor, which
/// Verter's own analysis does own, stays renameable.
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
// A classification owning no same-file range (`classify_rename_target` →
// `RenameTarget::same_file_ranges` empty) is not the same fact as the server
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

    // `{{ count }}`: `c` at delta 3 … `t` at delta 7, so the four named cases are
    // 3 / 5 / 7 / 8. They MUST be pairwise distinct — a duplicated delta silently
    // stops exercising the case its label claims.
    const TOKEN_OFFSETS: [(&str, usize); 4] = [
        ("first byte", "{{ ".len()),
        ("interior", "{{ co".len()),
        ("last byte", "{{ coun".len()),
        ("one past the last byte", "{{ count".len()),
    ];
    let deltas: Vec<usize> = TOKEN_OFFSETS.iter().map(|(_, delta)| *delta).collect();
    let mut distinct = deltas.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        deltas.len(),
        "each named boundary case must have its OWN offset, got {TOKEN_OFFSETS:?}"
    );
    assert_eq!(
        deltas,
        vec![3, 5, 7, 8],
        "first byte / interior / last byte / one-past must be exactly the token's \
         3 / 5 / 7 / 8 deltas"
    );

    for (label, delta) in TOKEN_OFFSETS {
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

// ── The CLIENT-VISIBLE prepare→rename handshake ─────────────────────────────
//
// The server advertises `prepareProvider: true`, so a real client sends
// `textDocument/prepareRename` FIRST and aborts on a `null` answer — it never
// sends `textDocument/rename` at all. A test that calls `rename` directly
// therefore greenlights a path no real client can reach. These lanes drive the
// handshake in the client's order: capability → prepareRename → rename.
//
// The prepare matrix at an instance-member position, one lane each:
//   provider ABSENT ................ `prepare_rename_refuses_the_instance_member_anchor_…`
//   provider EMPTY ................. `…_declines_when_the_provider_answers_empty`
//   provider ERROR ................. `…_declines_when_the_provider_errors`
//   unsafe mapping / geometry ...... `…_declines_when_the_provider_locations_do_not_map_onto_the_token`
//   safely mappable target ......... `prepare_rename_handshake_…` (offers the authored range)

/// The exact authored range of the interpolation's `count` token — what the
/// editor must be handed for the rename box, and nothing wider.
fn interpolation_token_range(source: &str) -> tower_lsp_server::ls_types::Range {
    let start = position_of(source, "{{ count }}", "{{ ".len());
    tower_lsp_server::ls_types::Range {
        start,
        end: Position {
            line: start.line,
            character: start.character + "count".len() as u32,
        },
    }
}

/// Arrange a mock provider that RESOLVES the instance member at `position`,
/// reporting its own occurrence (the `count` token of
/// `___VERTER___instance.count` in the generated TSX, which maps back onto the
/// interpolation). Returns the fixture and the provider.
async fn fixture_with_answering_provider(
    position: Position,
) -> (
    RenameFixture,
    Arc<crate::type_provider::mock::MockTypeProvider>,
) {
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_with_provider_kind(
        PLAIN_SCRIPT_SOURCE,
        Some(provider.clone()),
        crate::TypeProviderKind::None,
    )
    .await;
    let (tsx_path, query_offset) = provider_query_anchor(f.server(), &f.uri, position).await;
    let token_start = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ ".len());
    let (_, token_offset) = provider_query_anchor(f.server(), &f.uri, token_start).await;
    provider.set_rename_locations(
        &tsx_path,
        query_offset,
        vec![crate::type_provider::protocol::RenameLocation {
            path: tsx_path.clone(),
            start: token_offset,
            end: token_offset + "count".len() as u32,
        }],
    );
    (f, provider)
}

/// THE HANDSHAKE: `prepareProvider: true` is advertised, so an answering
/// provider's instance-member position must survive the client's prepare step
/// and produce a provider-owned edit — never the same-named script `const`.
///
/// Before this, prepare consulted only Verter's native analysis, so an
/// instance-member position answered `null`, VS Code aborted, and the provider —
/// the SOLE authority there — was never asked. Fail-closed means shipping no
/// edit when the provider cannot answer, not never asking.
#[tokio::test]
async fn prepare_rename_handshake_offers_the_instance_member_then_renames_only_provider_occurrences(
) {
    // (1) The capability the client keys its handshake on.
    let caps = crate::capabilities::server_capabilities(
        &tower_lsp_server::ls_types::PositionEncodingKind::UTF16,
        false,
    );
    match caps.rename_provider {
        Some(tower_lsp_server::ls_types::OneOf::Right(options)) => assert_eq!(
            options.prepare_provider,
            Some(true),
            "the handshake under test only happens because prepare is advertised"
        ),
        other => panic!("rename must advertise prepare support, got {other:?}"),
    }

    let position = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());
    let (f, provider) = fixture_with_answering_provider(position).await;

    // (2) + (3) prepareRename answers a NON-NULL, EXACT authored token range.
    let prepared = prepare_rename_at(f.server(), &f.uri, position).await;
    match prepared {
        Some(PrepareRenameResponse::Range(range)) => assert_eq!(
            range,
            interpolation_token_range(PLAIN_SCRIPT_SOURCE),
            "prepare must offer exactly the authored `count` token of the interpolation"
        ),
        other => panic!(
            "an answering provider owns this position, so prepare must offer its authored \
             range, got {other:?}"
        ),
    }
    assert!(
        provider.calls().iter().any(|call| matches!(
            call,
            crate::type_provider::mock::MockCall::GetRenameLocations { .. }
        )),
        "prepare must ASK the authority it defers to; calls: {:?}",
        provider.calls()
    );

    // (4) + (5) the follow-up rename ships ONLY the provider's own occurrence.
    let edit = rename_at(f.server(), &f.uri, position, "renamed")
        .await
        .expect("the rename the client sends after a non-null prepare must produce edits");
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

/// PROVIDER-EMPTY lane. An empty location set is not evidence of anything: it is
/// what a carrier denied by feature admission serves and what a provider that
/// resolved nothing serves. Prepare must decline — and must have ASKED, so this
/// is a refusal, not a short-circuit that never consulted the authority.
#[tokio::test]
async fn prepare_rename_at_an_instance_member_declines_when_the_provider_answers_empty() {
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_with_provider_kind(
        PLAIN_SCRIPT_SOURCE,
        Some(provider.clone()),
        crate::TypeProviderKind::None,
    )
    .await;
    let position = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());

    // No configured response: `get_rename_locations` answers `Ok(vec![])`.
    let prepared = prepare_rename_at(f.server(), &f.uri, position).await;

    assert!(
        prepared.is_none(),
        "an empty provider answer proves no rename target, so prepare must offer nothing, \
         got {prepared:?}"
    );
    assert!(
        provider.calls().iter().any(|call| matches!(
            call,
            crate::type_provider::mock::MockCall::GetRenameLocations { .. }
        )),
        "the provider owns this position, so prepare must actually ask it; calls: {:?}",
        provider.calls()
    );
    f.shutdown().await;
}

/// PROVIDER-ERROR lane (a transport/engine failure, and the same arm a timeout
/// lands on). Prepare declines exactly like the empty answer — never a guessed
/// range from the native name-based surface.
#[tokio::test]
async fn prepare_rename_at_an_instance_member_declines_when_the_provider_errors() {
    let inner = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let provider = Arc::new(RenameErrorProvider {
        inner: Arc::clone(&inner),
    });
    let f = fixture_with_provider_kind(
        PLAIN_SCRIPT_SOURCE,
        Some(provider.clone()),
        crate::TypeProviderKind::None,
    )
    .await;
    let position = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());

    let prepared = prepare_rename_at(f.server(), &f.uri, position).await;

    assert!(
        prepared.is_none(),
        "a failing provider proves nothing, so prepare must offer nothing, got {prepared:?}"
    );
    assert!(
        provider.rename_queries() > 0,
        "the error arm is only exercised if the query actually happened"
    );
    // And the follow-up rename a (hypothetically non-aborting) client could send
    // still ships nothing: the native `const` is never the fallback.
    let edit = rename_at(f.server(), &f.uri, position, "renamed").await;
    assert!(
        edit.is_none(),
        "a failing provider must not fall back to the script `const`, got {edit:?}"
    );
    f.shutdown().await;
}

/// UNSAFE-MAPPING lane. The provider answers, but its location cannot be mapped
/// back onto the authored token under the cursor (here: a location in a file the
/// request's carrier surface knows nothing about). An offer would promise the
/// editor a rename the transaction cannot deliver at that range, so prepare
/// declines.
///
/// The decline alone does not discriminate — a prepare that never consults the
/// provider at all declines too, which is exactly the behaviour this thread
/// replaced. So the lane also proves the query HAPPENED: this is a mapping
/// refusal over a real answer, not a short-circuit.
#[tokio::test]
async fn prepare_rename_declines_when_provider_locations_do_not_map_onto_the_authored_token() {
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_with_provider_kind(
        PLAIN_SCRIPT_SOURCE,
        Some(provider.clone()),
        crate::TypeProviderKind::None,
    )
    .await;
    let position = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());
    let (tsx_path, query_offset) = provider_query_anchor(f.server(), &f.uri, position).await;

    // A non-empty answer whose single location belongs to an UNRELATED path: it
    // maps onto no authored range in this document.
    provider.set_rename_locations(
        &tsx_path,
        query_offset,
        vec![crate::type_provider::protocol::RenameLocation {
            path: "/nonexistent/elsewhere.ts".to_string(),
            start: 0,
            end: 5,
        }],
    );

    let prepared = prepare_rename_at(f.server(), &f.uri, position).await;

    assert!(
        prepared.is_none(),
        "a location that does not map onto the authored token proves no renameable range \
         here, got {prepared:?}"
    );
    assert!(
        provider.calls().iter().any(|call| matches!(
            call,
            crate::type_provider::mock::MockCall::GetRenameLocations { .. }
        )),
        "the provider owns this position, so the decline must come from its UNMAPPABLE answer — \
         a prepare that never asked declines too and would prove nothing; calls: {:?}",
        provider.calls()
    );
    f.shutdown().await;
}

/// The authored carrier range a `[start, end)` span of the generated TSX maps
/// back onto — the exact conversion `provider_proves_rename_target` performs on
/// each provider location before comparing it to the anchor.
///
/// A lane that asserts a DECLINE needs this: "the provider's location does not
/// equal the anchor" and "the provider's location maps nowhere at all" produce
/// the same `None`, and only the first exercises the anchor comparison. Proving
/// the chosen location really maps is what makes such a lane discriminating.
async fn carrier_range_of_tsx_span(
    server: &VerterLanguageServer,
    uri: &Uri,
    start: u32,
    end: u32,
) -> Option<tower_lsp_server::ls_types::Range> {
    let ctx = server
        .repaired_type_provider_context(uri)
        .await
        .expect("the carrier has a provider context");
    crate::type_provider::merge::tsx_range_to_carrier_range(
        start,
        end,
        &ctx.tsx_line_index,
        &ctx.mapper,
        &ctx.carrier_line_index,
    )
}

/// EXACT-ANCHOR lane of the prepare matrix — the row the UNSAFE-MAPPING lane
/// above structurally cannot reach.
///
/// That lane's location sits on a FOREIGN path, so the path guard short-circuits
/// and the anchor comparison never runs at all: relaxing it to "some location
/// mapped somewhere" leaves that lane green. Here the location is on the
/// request's OWN companion and maps cleanly back onto a real authored range —
/// just a DIFFERENT one from the token under the cursor (the `:title="count"`
/// directive value, while the cursor is in the `{{ count }}` interpolation). So
/// every guard ahead of the comparison passes and the comparison alone decides.
///
/// MAPPABLE is not the same fact as OWNS THIS TOKEN. `prepare_rename` offers the
/// cursor's anchor, so a provider proving some other occurrence proves no
/// renameable range HERE — offering the anchor would promise the editor a rename
/// of a range the transaction never covers, which is exactly what
/// `instance_member_rename_refuses_a_provider_answer_that_edits_a_different_occurrence`
/// then refuses on the rename side.
#[tokio::test]
async fn prepare_rename_declines_when_the_provider_maps_onto_a_different_authored_token() {
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_with_provider_kind(
        PLAIN_SCRIPT_SOURCE,
        Some(provider.clone()),
        crate::TypeProviderKind::None,
    )
    .await;
    // The cursor is inside the INTERPOLATION, so the anchor is its token.
    let position = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());
    let anchor = interpolation_token_range(PLAIN_SCRIPT_SOURCE);
    let (tsx_path, query_offset) = provider_query_anchor(f.server(), &f.uri, position).await;

    // The provider answers with the OTHER template spelling of the same instance
    // member: same file, same companion path, genuinely mappable.
    let other_token = position_of(PLAIN_SCRIPT_SOURCE, ":title=\"count\"", ":title=\"".len());
    let (_, other_offset) = provider_query_anchor(f.server(), &f.uri, other_token).await;
    let other_end = other_offset + "count".len() as u32;

    // PRECONDITION — without this the decline below would not discriminate:
    // the location must map, and must map somewhere OTHER than the anchor.
    let mapped = carrier_range_of_tsx_span(f.server(), &f.uri, other_offset, other_end).await;
    assert_eq!(
        mapped.map(|r| (r.start.line, r.start.character)),
        Some(TEMPLATE_OCCURRENCES[0]),
        "precondition: the chosen location must map onto the `:title` occurrence — if it mapped \
         NOWHERE the decline would be an unmappable-answer refusal and would prove nothing about \
         the anchor comparison; got {mapped:?}"
    );
    assert_ne!(
        mapped,
        Some(anchor),
        "precondition: it must map somewhere OTHER than the anchor, or there is no distinction \
         between `== Some(anchor)` and `is_some()` to test"
    );

    provider.set_rename_locations(
        &tsx_path,
        query_offset,
        vec![crate::type_provider::protocol::RenameLocation {
            path: tsx_path.clone(),
            start: other_offset,
            end: other_end,
        }],
    );

    let prepared = prepare_rename_at(f.server(), &f.uri, position).await;

    assert!(
        prepared.is_none(),
        "the provider proved an occurrence it does not share with the token under the cursor, so \
         prepare must offer NOTHING — offering `{anchor:?}` would open a rename box for a range \
         the follow-up transaction refuses; got {prepared:?}"
    );
    assert!(
        provider.calls().iter().any(|call| matches!(
            call,
            crate::type_provider::mock::MockCall::GetRenameLocations { .. }
        )),
        "the decline must come from the provider's MAPPED-BUT-WRONG answer — a prepare that never \
         asked declines too and would prove nothing; calls: {:?}",
        provider.calls()
    );
    f.shutdown().await;
}

/// PREPARE IS NOT AUTHORITY TRANSFERABLE ACROSS A RACE. A prepare that said yes
/// does not license the follow-up rename to skip its own checks: rename
/// re-resolves the cursor and re-queries the provider against the CURRENT
/// document. Here the document changes between the two calls, so the provider no
/// longer answers for the moved anchor — and rename must ship nothing rather
/// than honour the earlier yes (or fall back to the script `const`).
#[tokio::test]
async fn rename_revalidates_after_a_yes_prepare_and_ships_nothing_when_the_answer_moved() {
    let position = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());
    let (f, provider) = fixture_with_answering_provider(position).await;
    let rename_queries = |provider: &crate::type_provider::mock::MockTypeProvider| {
        provider
            .calls()
            .iter()
            .filter(|call| {
                matches!(
                    call,
                    crate::type_provider::mock::MockCall::GetRenameLocations { .. }
                )
            })
            .count()
    };

    let prepared = prepare_rename_at(f.server(), &f.uri, position).await;
    assert!(
        prepared.is_some(),
        "precondition: the provider answers, so prepare offers a range"
    );
    let queries_after_prepare = rename_queries(&provider);
    assert!(
        queries_after_prepare >= 1,
        "precondition: prepare asked the authority"
    );

    // The buffer changes: a new leading line shifts every authored offset, so the
    // generated-TSX offset the provider was scripted for no longer corresponds to
    // the cursor. The provider generation has effectively moved on.
    const SHIFTED_SOURCE: &str = "<script lang=\"ts\">\n// a new first line\nconst count = 1\nconsole.log(count)\n</script>\n\n<template>\n<div :title=\"count\">{{ count }}</div>\n</template>\n";
    let _ = f.server().documents.did_change(&f.uri, 2, SHIFTED_SOURCE);
    let moved_position = position_of(SHIFTED_SOURCE, "{{ count }}", "{{ co".len());

    let edit = rename_at(f.server(), &f.uri, moved_position, "renamed").await;

    assert!(
        edit.is_none(),
        "rename must re-prove the target itself; a stale prepare must not license an edit, \
         got {edit:?}"
    );
    // Rename issued its OWN provider query against the changed document — it did
    // not reuse the answer prepare already had. That answer belonged to the
    // PREVIOUS document, so nothing at the moved cursor establishes provider
    // completeness, and Verter's own half claims no range at an instance member:
    // `handle_rename`'s completeness refusal is what produces the `None`, the
    // gate a stale yes-prepare must never be able to skip.
    assert!(
        rename_queries(&provider) > queries_after_prepare,
        "rename must re-ask the provider itself (was {queries_after_prepare}, now {}); \
         prepare's answer is not transferable",
        rename_queries(&provider)
    );
    f.shutdown().await;
}

/// A NON-EMPTY provider answer is not a completeness proof. Verter owns no
/// same-file occurrence at a provider-only instance member — the same-named
/// script `const` is a different symbol — but it does own the AUTHORED TOKEN
/// under the cursor. A transaction that renames some OTHER occurrence and leaves
/// that token alone renamed something the user did not ask for: here the
/// provider's location maps onto the `:title="count"` directive value, a
/// different instance-member access from the interpolation the cursor is in.
///
/// `prepare_rename` already refuses to offer a range unless a provider location
/// maps back EXACTLY onto the authored token
/// (`provider_proves_rename_target`). The follow-up rename must hold the EMITTED
/// transaction to that same range, or a client that skips prepare — or a provider
/// whose answer moved between the two calls — gets the mis-anchored edit shipped
/// as a success.
#[tokio::test]
async fn instance_member_rename_refuses_a_provider_answer_that_edits_a_different_occurrence() {
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_with_provider_kind(
        PLAIN_SCRIPT_SOURCE,
        Some(provider.clone()),
        crate::TypeProviderKind::None,
    )
    .await;
    let position = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());
    let (tsx_path, query_offset) = provider_query_anchor(f.server(), &f.uri, position).await;

    // The reported location is the OTHER instance-member access in the same file.
    let other_token = position_of(PLAIN_SCRIPT_SOURCE, ":title=\"count\"", ":title=\"".len());
    let (_, other_offset) = provider_query_anchor(f.server(), &f.uri, other_token).await;
    provider.set_rename_locations(
        &tsx_path,
        query_offset,
        vec![crate::type_provider::protocol::RenameLocation {
            path: tsx_path.clone(),
            start: other_offset,
            end: other_offset + "count".len() as u32,
        }],
    );

    let edit = rename_at(f.server(), &f.uri, position, "renamed").await;

    assert!(
        edit.is_none(),
        "a transaction that does not overwrite the authored token under the cursor renamed \
         something else and must not ship; got edits at {:?}",
        edit.as_ref().map(|e| edit_starts(e, &f.uri))
    );
    assert!(
        provider.calls().iter().any(|call| matches!(
            call,
            crate::type_provider::mock::MockCall::GetRenameLocations { .. }
        )),
        "this is a completeness refusal over a REAL answer, not a short-circuit that never \
         asked; calls: {:?}",
        provider.calls()
    );
    f.shutdown().await;
}

/// Arrange a mock provider that answers `position` with exactly `locations`.
///
/// [`crate::type_provider::mock::MockTypeProvider::set_rename_locations`] keeps
/// the FIRST registration for a `(path, offset)` pair, so a lane that needs two
/// different answers for the same anchor needs two fixtures — hence a helper
/// rather than a re-registration. Returns the fixture plus the queried
/// `{carrier}.tsx` path and the authored token's generated offset, so the caller
/// builds its location set from the same anchors the handler resolves.
async fn fixture_answering_with(
    position: Position,
    locations: impl FnOnce(&str, u32) -> Vec<crate::type_provider::protocol::RenameLocation>,
) -> RenameFixture {
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_with_provider_kind(
        PLAIN_SCRIPT_SOURCE,
        Some(provider.clone()),
        crate::TypeProviderKind::None,
    )
    .await;
    let (tsx_path, query_offset) = provider_query_anchor(f.server(), &f.uri, position).await;
    let token_start = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ ".len());
    let (_, token_offset) = provider_query_anchor(f.server(), &f.uri, token_start).await;
    provider.set_rename_locations(&tsx_path, query_offset, locations(&tsx_path, token_offset));
    f
}

/// The provider's own occurrence in the CURRENT companion, which maps back onto
/// the interpolation — the leg that on its own is a complete transaction.
fn current_companion_leg(
    tsx_path: &str,
    token_offset: u32,
) -> crate::type_provider::protocol::RenameLocation {
    crate::type_provider::protocol::RenameLocation {
        path: tsx_path.to_string(),
        start: token_offset,
        end: token_offset + "count".len() as u32,
    }
}

/// A location in a FOREIGN carrier IDE companion. Only that file's OWN source map
/// could map its offsets onto authored bytes, and this request pinned no surface
/// for it, so the merge can produce no edit for it at all — the shape a live
/// foreign carrier whose buffer no longer byte-matches the captured surface takes
/// (`foreign_ide_context_from_captured` answers `None`).
fn unmappable_foreign_companion_leg(
    tsx_path: &str,
) -> crate::type_provider::protocol::RenameLocation {
    let foreign = tsx_path.replace("App.vue.tsx", "Sibling.vue.tsx");
    assert_ne!(
        foreign, tsx_path,
        "the foreign leg must target a DIFFERENT companion than the queried one, else it takes \
         the in-context mapper and this lane proves nothing"
    );
    crate::type_provider::protocol::RenameLocation {
        path: foreign,
        start: 0,
        end: "count".len() as u32,
    }
}

/// A provider answer the merge can only PARTLY map is an INCOMPLETE transaction,
/// not a smaller one.
///
/// The merge maps each provider location onto authored bytes and, when it cannot,
/// produces no edit for it. Cross-file legs are exactly where that happens for
/// reasons the caller cannot see: a foreign carrier companion is mappable only
/// through its OWN pinned source map, so a foreign carrier whose live buffer no
/// longer byte-matches the captured surface, a superseded generation, and an
/// uncaptured path all yield nothing. Shipping the remaining edits renames the
/// symbol HERE and leaves every unmapped file referencing a name that no longer
/// exists — a dangling reference delivered as a success, which is the write-side
/// twin of the partials the same-file gate closes.
///
/// The same-file gate cannot see this: it proves the emitted set covers the
/// authored occurrences of THIS file, which the surviving leg does. So the merge
/// reports the locations it dropped and the handler refuses the WHOLE
/// transaction.
///
/// DISCRIMINATING, and the positive control is the point: the two lanes differ by
/// exactly ONE added unmappable location. The first ships the mapped edit, so the
/// refusal cannot be a blanket "any provider location fails" nor an artifact of
/// the fixture.
#[tokio::test]
async fn rename_refuses_when_the_merge_drops_a_provider_location() {
    let position = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());

    // POSITIVE CONTROL: the mappable leg alone is a complete transaction.
    let mappable = fixture_answering_with(position, |tsx_path, token_offset| {
        vec![current_companion_leg(tsx_path, token_offset)]
    })
    .await;
    let complete = rename_at(mappable.server(), &mappable.uri, position, "renamed")
        .await
        .expect("precondition: a fully mappable provider answer renames");
    assert_eq!(
        edit_starts(&complete, &mappable.uri),
        vec![TEMPLATE_OCCURRENCES[1]],
        "precondition: the mappable leg edits exactly the interpolation"
    );
    mappable.shutdown().await;

    // The SAME answer plus ONE unmappable cross-file leg. Nothing about the
    // mappable leg changed, so an edit that still ships is a partial rename.
    let partial = fixture_answering_with(position, |tsx_path, token_offset| {
        vec![
            current_companion_leg(tsx_path, token_offset),
            unmappable_foreign_companion_leg(tsx_path),
        ]
    })
    .await;
    let edit = rename_at(partial.server(), &partial.uri, position, "renamed").await;

    assert!(
        edit.is_none(),
        "a provider location the merge could not map must fail the WHOLE rename closed — never \
         the same-file half, which would leave the foreign carrier dangling; got edits at {:?}",
        edit.as_ref().map(|e| edit_starts(e, &partial.uri))
    );
    partial.shutdown().await;
}

/// A drop on the CURRENT companion is delegated to the same-file gate ONLY when
/// that gate's proof enumerates the file — the delegation is conditioned on PROOF
/// COMPLETENESS, never on path identity alone.
///
/// An unmapped offset in the generated projection is what SYNTHETIC generated code
/// looks like: Verter's IDE surface re-spells authored bindings in unmapped
/// constructs (the setup-return shim emits
/// `doubled: doubled as unknown as typeof doubled`), a real provider reports every
/// one of them, and nothing in the `.vue` corresponds to them. Treating those as
/// incompleteness would refuse essentially every real rename. But the delegation
/// only means something if
/// [`same_file_rename_is_complete`](super::nav_features_navigation_rename_gate::same_file_rename_is_complete)
/// re-requires every authored occurrence in that file; with a strict-subset proof
/// the drop is behind NO gate and must refuse.
///
/// A drop anywhere ELSE is covered by no gate in either case.
///
/// The identity test is filesystem identity, not string equality: a provider that
/// spells the companion path differently must still be recognised as the current
/// file, or the rename refuses for a spelling difference.
#[test]
fn the_drop_gate_delegates_the_current_companion_only_under_a_whole_file_proof() {
    use super::super::rename_plan::SameFileProof;
    use super::nav_features_navigation_rename_gate::unguarded_rename_drops;
    use crate::type_provider::merge::{DroppedRenameLocation, RenameDropReason};
    use tower_lsp_server::ls_types::Range;

    let current = "/w/src/App.vue.tsx";
    let whole_file = SameFileProof::Requires {
        ranges: vec![Range {
            start: Position {
                line: 1,
                character: 6,
            },
            end: Position {
                line: 1,
                character: 11,
            },
        }],
        enumerates_whole_file: true,
    };
    // Same required range, but NOT asserted to be the file's whole occurrence set
    // — a provider-only anchor, an unclassifiable position, or a carrier with no
    // markup occurrence inventory.
    let subset = SameFileProof::Requires {
        ranges: match &whole_file {
            SameFileProof::Requires { ranges, .. } => ranges.clone(),
            SameFileProof::Unprovable => unreachable!(),
        },
        enumerates_whole_file: false,
    };
    let synthetic_shim = DroppedRenameLocation {
        path: current.to_string(),
        start: 3690,
        end: 3697,
        reason: RenameDropReason::CarrierIdeUnmapped,
    };
    let foreign_companion = DroppedRenameLocation {
        path: "/w/src/Sibling.vue.tsx".to_string(),
        start: 12,
        end: 19,
        reason: RenameDropReason::CarrierIdeUnmapped,
    };
    let real_file = DroppedRenameLocation {
        path: "/w/src/helpers.ts".to_string(),
        start: 40,
        end: 47,
        reason: RenameDropReason::TargetSourceUnreadable,
    };

    assert!(
        unguarded_rename_drops(std::slice::from_ref(&synthetic_shim), current, &whole_file)
            .is_empty(),
        "under a WHOLE-FILE proof the same-file gate owns this file, so an unmapped companion \
         offset is delegated — refusing here would refuse every real rename"
    );
    // THE CONDITION. Byte-identical drop, byte-identical path, and the ONLY
    // difference is that the proof no longer claims to enumerate the file.
    assert_eq!(
        unguarded_rename_drops(std::slice::from_ref(&synthetic_shim), current, &subset),
        vec![&synthetic_shim],
        "with a strict-SUBSET proof the same-file gate cannot see what the drop hid, so the \
         delegation has no substance and the drop must reach the refusal"
    );
    assert!(
        unguarded_rename_drops(
            std::slice::from_ref(&synthetic_shim),
            current,
            &SameFileProof::Unprovable
        )
        .len()
            == 1,
        "`Unprovable` proves nothing and vouches for nothing"
    );
    // The SAME location, spelled with the OS-alternate separator: filesystem
    // identity, not `==`.
    assert!(
        unguarded_rename_drops(
            &[DroppedRenameLocation {
                path: current.replace('/', "\\"),
                ..synthetic_shim.clone()
            }],
            current,
            &whole_file,
        )
        .is_empty(),
        "a differently-spelled path for the SAME companion must not read as another file"
    );
    assert_eq!(
        unguarded_rename_drops(
            &[
                synthetic_shim.clone(),
                foreign_companion.clone(),
                real_file.clone()
            ],
            current,
            &whole_file,
        ),
        vec![&foreign_companion, &real_file],
        "every file the transaction does not edit and no gate proves must reach the refusal — \
         and only those"
    );
}

// ── `<style>` `v-bind()` — the TERMINAL same-file shortfall, end to end ──────
//
// The gate lane above stages its `SameFileProof` by hand, so it proves the gate
// honours a strict-subset proof but says nothing about which authored shapes
// PRODUCE one. `claim_enumeration_without_markup` is the only producer of the
// terminal `Partial(StyleVBindExpression)` arm, and it is reachable only through
// a real `<style>` `v-bind()` in a real carrier — so it needs a lane on the real
// LSP path.

/// A `<script setup>` binding that a `<style>` `v-bind()` expression references.
/// `count` is spelled in the script, in BOTH template regions, and in the style
/// expression; `other` exists so the sibling source below can differ by the
/// v-bind's NAME alone.
const STYLE_VBIND_SOURCE: &str = "<script setup lang=\"ts\">\nconst count = 1\nconst other = 2\nconsole.log(count, other)\n</script>\n\n<template>\n<div :title=\"count\">{{ count }}</div>\n</template>\n\n<style>\n.a { width: v-bind(count); }\n</style>\n";

/// BYTE-IDENTICAL to [`STYLE_VBIND_SOURCE`] except that the `v-bind()` names
/// `other` instead of `count`. Same script, same template, same style rule, same
/// block structure — so any behavioural difference between the two is the
/// per-NAME `style_vbind_roots` decision and nothing else.
const STYLE_VBIND_UNRELATED_SOURCE: &str = "<script setup lang=\"ts\">\nconst count = 1\nconst other = 2\nconsole.log(count, other)\n</script>\n\n<template>\n<div :title=\"count\">{{ count }}</div>\n</template>\n\n<style>\n.a { width: v-bind(other); }\n</style>\n";

/// Every authored occurrence of `count` in [`STYLE_VBIND_SOURCE`] that Verter's
/// native span set proves: the declaration, the `console.log` usage, and both
/// template spellings.
const STYLE_VBIND_NATIVE_OCCURRENCES: [(u32, u32); 4] = [(1, 6), (3, 12), (7, 13), (7, 23)];

/// The `count` token INSIDE `v-bind(count)` — an authored occurrence of the same
/// binding that is in NO Verter inventory. This is the byte the refusal exists
/// to protect.
const STYLE_VBIND_OCCURRENCE: (u32, u32) = (11, 19);

/// `rename` as a `Result`, so a lane can assert the FAIL-CLOSED ERROR arm.
/// [`rename_at`] unwraps and would panic on it.
async fn rename_result_at(
    server: &VerterLanguageServer,
    uri: &Uri,
    position: Position,
    new_name: &str,
) -> tower_lsp_server::jsonrpc::Result<Option<tower_lsp_server::ls_types::WorkspaceEdit>> {
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
}

/// A fixture over `source` whose provider answers the rename query at `position`
/// with ONE location on the request's OWN companion at an offset the generated
/// projection cannot map — the shape of the synthetic re-spellings Verter's IDE
/// surface emits (`doubled: doubled as unknown as typeof doubled`), which a real
/// provider reports and which map to no authored byte.
///
/// That drop is delegated to the same-file gate ONLY under a whole-file proof, so
/// it is the exact probe that reads out whether this source's claim enumerates
/// the file.
async fn fixture_with_an_unmappable_companion_drop(
    source: &str,
    position: Position,
) -> RenameFixture {
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_with_provider_kind(
        source,
        Some(provider.clone()),
        crate::TypeProviderKind::None,
    )
    .await;
    let (tsx_path, query_offset) = provider_query_anchor(f.server(), &f.uri, position).await;
    // Far past the end of the generated TSX: the in-context mapper resolves no
    // carrier range, so the merge records it as `CarrierIdeUnmapped`.
    let unmappable_start = 9_000_000;
    assert!(
        carrier_range_of_tsx_span(
            f.server(),
            &f.uri,
            unmappable_start,
            unmappable_start + "count".len() as u32,
        )
        .await
        .is_none(),
        "precondition: the probe offset must be UNMAPPABLE, or it becomes an ordinary edit and \
         the lane exercises no drop at all"
    );
    provider.set_rename_locations(
        &tsx_path,
        query_offset,
        vec![crate::type_provider::protocol::RenameLocation {
            path: tsx_path.clone(),
            start: unmappable_start,
            end: unmappable_start + "count".len() as u32,
        }],
    );
    f
}

/// A `<style>` `v-bind(count)` expression is an authored occurrence of `count`
/// that Verter's rename span set NEVER carries, so a claim over `count` in this
/// file is permanently a strict subset — and a rename that cannot prove itself
/// complete must SAY SO rather than silently no-op.
///
/// Three legs, and the first two are why the third means anything:
///
/// 1. THE PREMISE. With no provider the native half IS the transaction, and it
///    edits the script and both template regions while leaving `v-bind(count)`
///    untouched. That is the shortfall in bytes: the refusal is not defensive
///    book-keeping, there really is an authored occurrence no emitted edit
///    covers.
/// 2. THE REFUSAL. Add a provider whose answer carries an unmapped offset in the
///    current companion. That drop is delegated to the same-file gate only under
///    a WHOLE-FILE proof, which this file can never produce — so the rename fails
///    closed, and it returns the user-visible reason rather than `Ok(None)`.
/// 3. THE CONTROL. The same source and the same provider answer, with the
///    `v-bind()` naming `other` instead of `count`. The claim is then complete,
///    the identical drop is delegated, and the identical rename SHIPS. So the
///    refusal is neither "any `<style>` block blocks rename" nor "any unmapped
///    offset blocks rename": it is decided per NAME, exactly as
///    `claim_enumeration_without_markup` claims.
#[tokio::test]
async fn a_style_v_bind_reference_refuses_the_rename_and_says_why() {
    let position = position_of(STYLE_VBIND_SOURCE, "const count", "const ".len());
    assert_eq!(
        (position.line, position.character),
        STYLE_VBIND_NATIVE_OCCURRENCES[0],
        "the cursor sits on the declaration"
    );

    // ── 1. THE PREMISE: the native span set omits the style expression ───────
    let native_only = fixture(STYLE_VBIND_SOURCE).await;
    let native = rename_at(native_only.server(), &native_only.uri, position, "renamed")
        .await
        .expect("with no provider the native half is the whole transaction");
    let native_starts = edit_starts(&native, &native_only.uri);
    assert_eq!(
        native_starts,
        STYLE_VBIND_NATIVE_OCCURRENCES.to_vec(),
        "the native inventory is the script + template occurrences"
    );
    assert!(
        !native_starts.contains(&STYLE_VBIND_OCCURRENCE),
        "THE SHORTFALL: `v-bind(count)` at {STYLE_VBIND_OCCURRENCE:?} is an authored occurrence \
         of the very same binding, and no emitted edit covers it — this is what a satisfied \
         same-file claim would leave behind, and why the claim must not call itself complete; \
         got {native_starts:?}"
    );
    native_only.shutdown().await;

    // ── 2. THE REFUSAL, with its reason ─────────────────────────────────────
    let refusing = fixture_with_an_unmappable_companion_drop(STYLE_VBIND_SOURCE, position).await;
    let refused = rename_result_at(refusing.server(), &refusing.uri, position, "renamed").await;

    let error = match refused {
        Err(error) => error,
        Ok(edit) => panic!(
            "a companion drop this file's claim cannot vouch for must fail the rename CLOSED, \
             got {:?}",
            edit.as_ref().map(|e| edit_starts(e, &refusing.uri))
        ),
    };
    assert!(
        error.message.contains("v-bind()"),
        "the refusal must name the construct the user has to act on, or it is no more useful \
         than the silent `Ok(None)` it replaced; got {:?}",
        error.message
    );
    assert!(
        error.message.starts_with("verter: rename is unavailable"),
        "same shape as the multi-claimant refusal — the one existing user-visible rename \
         fail-closed message; got {:?}",
        error.message
    );
    assert_eq!(
        error.code,
        tower_lsp_server::jsonrpc::ErrorCode::ServerError(-32803),
        "LSP `RequestFailed`, as the multi-claimant refusal uses: a known user-facing reason, \
         not a protocol fault"
    );
    refusing.shutdown().await;

    // ── 3. THE CONTROL: identical but for the v-bind's NAME ─────────────────
    let control_position = position_of(STYLE_VBIND_UNRELATED_SOURCE, "const count", "const ".len());
    assert_eq!(
        control_position, position,
        "the two sources must agree on the cursor, or the legs are not comparable"
    );
    let control =
        fixture_with_an_unmappable_companion_drop(STYLE_VBIND_UNRELATED_SOURCE, position).await;
    let shipped = rename_result_at(control.server(), &control.uri, position, "renamed")
        .await
        .expect("a v-bind naming a DIFFERENT binding leaves this claim complete")
        .expect("so the identical companion drop is delegated and the rename ships");
    assert_eq!(
        edit_starts(&shipped, &control.uri),
        STYLE_VBIND_NATIVE_OCCURRENCES.to_vec(),
        "the control must really SHIP the same edit set — otherwise leg 2's refusal could be an \
         artifact of the unmappable drop rather than of the style reference"
    );
    control.shutdown().await;
}

/// The two [`SameFileProof`](super::super::rename_plan::SameFileProof) arms are
/// NOT interchangeable, and the difference is the whole point: an EMPTY
/// `Requires` asserts nothing (a position Verter cannot classify must not suppress
/// a provider-owned result), while `Unprovable` refuses. A provider-only instance
/// member whose authored token does not convert to a range reaches the second
/// arm, which the LSP boundary cannot stage, so it is asserted directly.
#[test]
fn the_unprovable_same_file_proof_refuses_an_edit_an_empty_proof_would_admit() {
    use super::super::child_prop_rename::ChildPropRenameClass;
    use super::super::rename_plan::SameFileProof;
    use super::nav_features_navigation_rename_gate::same_file_rename_is_complete;
    use tower_lsp_server::ls_types::{Range, TextEdit, WorkspaceEdit};

    let uri: Uri = "file:///w/src/App.vue".parse().expect("uri");
    let range = Range {
        start: Position {
            line: 6,
            character: 23,
        },
        end: Position {
            line: 6,
            character: 28,
        },
    };
    #[allow(clippy::mutable_key_type)]
    let mut changes = std::collections::HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range,
            new_text: "renamed".to_string(),
        }],
    );
    let edit = WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    };

    assert!(
        same_file_rename_is_complete(
            Some(&edit),
            &uri,
            &SameFileProof::Requires {
                ranges: Vec::new(),
                enumerates_whole_file: false,
            },
            "renamed",
            &ChildPropRenameClass::NotChildProp,
        ),
        "an empty proof set asserts nothing and must admit the provider-owned edit"
    );
    assert!(
        !same_file_rename_is_complete(
            Some(&edit),
            &uri,
            &SameFileProof::Unprovable,
            "renamed",
            &ChildPropRenameClass::NotChildProp,
        ),
        "`Unprovable` must refuse the very same edit — nothing about the position was proven"
    );
}

// ── The current-companion drop exemption is conditioned on PROOF COMPLETENESS ─
//
// `unguarded_rename_drops` delegates a drop on the request's OWN provider
// companion to the same-file gate instead of refusing. That delegation is sound
// only where Verter's same-file proof is COMPLETE — where it re-requires EVERY
// authored occurrence in the file, so an authored occurrence hidden behind a
// dropped companion leg surfaces as a missing required range. Where the proof is
// a strict SUBSET of the file's authored occurrences, nothing covers that drop
// and the remainder ships as a partial rename.
//
// A `ProviderOnlyInstanceMember` proof is the cursor token ALONE, so it is such
// a subset: the file's OTHER instance-member spelling of the same name is not in
// it. These lanes drive that, and pair every refusal with a positive control
// proving the healthy path still renames — an exemption narrowed into
// "fail closed on any drop" would refuse both.

/// The byte offset of the `nth` occurrence of `needle` in the document's
/// GENERATED companion, plus the companion path — the coordinates a provider
/// reports its rename locations in.
async fn companion_offset_of(
    server: &VerterLanguageServer,
    uri: &Uri,
    needle: &str,
    nth: usize,
) -> (String, u32) {
    server.ensure_current_file_synced(uri).await;
    let ctx = server
        .repaired_type_provider_context(uri)
        .await
        .expect("the carrier has a provider context");
    let ide = server
        .documents
        .get_ide(uri)
        .expect("the carrier projects a generated companion");
    let offset = ide
        .code
        .match_indices(needle)
        .nth(nth)
        .unwrap_or_else(|| {
            panic!(
                "the generated companion must spell `{needle}` at least {} time(s):\n{}",
                nth + 1,
                ide.code
            )
        })
        .0;
    (ctx.tsx_path.clone(), offset as u32)
}

/// The two provider rename locations for `count` at an instance-member cursor:
/// the interpolation leg (mapped) and the DIRECTIVE-VALUE leg. Both are authored
/// occurrences of the SAME instance property — `:title="count"` and
/// `{{ count }}` both lower to `___VERTER___instance.count` — so a transaction
/// that renames one and not the other leaves the file referencing a name that no
/// longer exists.
async fn instance_member_legs(
    server: &VerterLanguageServer,
    uri: &Uri,
    directive_leg_maps: bool,
) -> (
    String,
    u32,
    Vec<crate::type_provider::protocol::RenameLocation>,
) {
    let cursor = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());
    let (tsx_path, query_offset) = provider_query_anchor(server, uri, cursor).await;
    let interpolation_token = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ ".len());
    let (_, interpolation_offset) = provider_query_anchor(server, uri, interpolation_token).await;

    // The DIRECTIVE-VALUE occurrence in the companion: the FIRST
    // `___VERTER___instance.count` is the `:title` projection, the second is the
    // interpolation.
    let (_, directive_member_offset) =
        companion_offset_of(server, uri, "___VERTER___instance.count", 0).await;
    let directive_count_offset = directive_member_offset + "___VERTER___instance.".len() as u32;
    // A mapped leg spans exactly the authored token; the REJECTED leg is the one
    // the provider ranged over the generated member expression, whose start byte
    // is an inserted `___VERTER___instance` the strict mapper bridges to no
    // authored byte.
    let directive = if directive_leg_maps {
        crate::type_provider::protocol::RenameLocation {
            path: tsx_path.clone(),
            start: directive_count_offset,
            end: directive_count_offset + "count".len() as u32,
        }
    } else {
        crate::type_provider::protocol::RenameLocation {
            path: tsx_path.clone(),
            start: directive_member_offset,
            end: directive_count_offset + "count".len() as u32,
        }
    };
    let locations = vec![
        crate::type_provider::protocol::RenameLocation {
            path: tsx_path.clone(),
            start: interpolation_offset,
            end: interpolation_offset + "count".len() as u32,
        },
        directive,
    ];
    (tsx_path, query_offset, locations)
}

/// POSITIVE CONTROL. Both authored instance-member occurrences map, so the
/// transaction covers both and ships. This is the rename the narrowed exemption
/// must keep serving — a "fail closed on ANY current-companion drop" rule would
/// refuse it, and so would a completeness predicate that classifies a healthy
/// Vue carrier as incomplete.
#[tokio::test]
async fn instance_member_rename_ships_both_authored_occurrences_when_both_legs_map() {
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_with_provider_kind(
        PLAIN_SCRIPT_SOURCE,
        Some(provider.clone()),
        crate::TypeProviderKind::None,
    )
    .await;
    let cursor = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());
    let (tsx_path, query_offset, locations) = instance_member_legs(f.server(), &f.uri, true).await;
    provider.set_rename_locations(&tsx_path, query_offset, locations);

    let edit = rename_at(f.server(), &f.uri, cursor, "renamed")
        .await
        .expect("both authored occurrences map, so the transaction is complete and must ship");

    let mut expected = TEMPLATE_OCCURRENCES.to_vec();
    expected.sort_unstable();
    assert_eq!(
        edit_starts(&edit, &f.uri),
        expected,
        "the transaction must cover BOTH instance-member spellings"
    );
    for script_start in SCRIPT_OCCURRENCES {
        assert!(
            !edit_starts(&edit, &f.uri).contains(&script_start),
            "the module-level `const` at {script_start:?} is a different symbol"
        );
    }
    f.shutdown().await;
}

/// A current-companion drop must NOT be exempt when the same-file proof is a
/// strict SUBSET of the file's authored occurrences.
///
/// Here the provider names BOTH authored occurrences of the instance property,
/// but ranges the directive-value leg over the generated member expression, so
/// the strict mapper rejects it and the merge drops it on the CURRENT companion.
/// The `ProviderOnlyInstanceMember` proof is the cursor token alone, so the
/// same-file gate — the authority the exemption delegates to — never looks at
/// `:title="count"`. The transaction would rename `{{ count }}` and leave
/// `:title="count"` bound to the old name, shipped as a success.
#[tokio::test]
async fn instance_member_rename_refuses_a_dropped_companion_leg_the_anchor_proof_cannot_see() {
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_with_provider_kind(
        PLAIN_SCRIPT_SOURCE,
        Some(provider.clone()),
        crate::TypeProviderKind::None,
    )
    .await;
    let cursor = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());
    let (tsx_path, query_offset, locations) = instance_member_legs(f.server(), &f.uri, false).await;
    provider.set_rename_locations(&tsx_path, query_offset, locations);

    let edit = rename_at(f.server(), &f.uri, cursor, "renamed").await;

    assert!(
        edit.is_none(),
        "the directive-value leg was dropped on the current companion and the anchor-only proof \
         cannot see it, so the whole rename must fail closed; got edits at {:?} (the authored \
         `:title=\"count\"` at {:?} would keep the old name)",
        edit.as_ref().map(|e| edit_starts(e, &f.uri)),
        TEMPLATE_OCCURRENCES[0]
    );
    f.shutdown().await;
}

/// A position Verter cannot classify at all (`RenameTargetClass::Unavailable`)
/// owns NO occurrence inventory, so its same-file proof asserts nothing. That is
/// not a completeness claim and must not exempt a current-companion drop.
///
/// A TYPE-ONLY import binding is such a position on a coherent, reachable path:
/// `classify_rename_target` excludes type-only imports from the native surface,
/// and no CSS name owns the offset, so the target is `Unavailable` while the
/// provider legitimately owns the rename.
#[tokio::test]
async fn type_only_import_rename_refuses_a_dropped_companion_leg_it_proves_nothing_about() {
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_with_provider_kind(
        TYPE_ONLY_IMPORT_SOURCE,
        Some(provider.clone()),
        crate::TypeProviderKind::None,
    )
    .await;
    let cursor = position_of(
        TYPE_ONLY_IMPORT_SOURCE,
        "import type { Widget }",
        "import type { Wi".len(),
    );
    let (tsx_path, query_offset) = provider_query_anchor(f.server(), &f.uri, cursor).await;
    let (_, unmapped) = companion_offset_of(f.server(), &f.uri, "___VERTER___instance", 0).await;

    // One mapped leg (the authored import binding) plus a leg the strict mapper
    // rejects on the CURRENT companion.
    let token = position_of(
        TYPE_ONLY_IMPORT_SOURCE,
        "import type { Widget }",
        "import type { ".len(),
    );
    let (_, token_offset) = provider_query_anchor(f.server(), &f.uri, token).await;
    provider.set_rename_locations(
        &tsx_path,
        query_offset,
        vec![
            crate::type_provider::protocol::RenameLocation {
                path: tsx_path.clone(),
                start: token_offset,
                end: token_offset + "Widget".len() as u32,
            },
            crate::type_provider::protocol::RenameLocation {
                path: tsx_path.clone(),
                start: unmapped,
                end: unmapped + "___VERTER___instance".len() as u32,
            },
        ],
    );

    let edit = rename_at(f.server(), &f.uri, cursor, "Renamed").await;

    assert!(
        edit.is_none(),
        "an `Unavailable` position proves nothing about this file, so it cannot vouch for a \
         dropped companion leg; got edits at {:?}",
        edit.as_ref().map(|e| edit_starts(e, &f.uri))
    );
    f.shutdown().await;
}

/// POSITIVE CONTROL for the lane above: with NO drop, the same `Unavailable`
/// position still serves the provider's own result. Verter must not suppress a
/// provider-owned rename at a position it cannot classify.
#[tokio::test]
async fn type_only_import_rename_still_serves_the_provider_when_nothing_is_dropped() {
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_with_provider_kind(
        TYPE_ONLY_IMPORT_SOURCE,
        Some(provider.clone()),
        crate::TypeProviderKind::None,
    )
    .await;
    let cursor = position_of(
        TYPE_ONLY_IMPORT_SOURCE,
        "import type { Widget }",
        "import type { Wi".len(),
    );
    let (tsx_path, query_offset) = provider_query_anchor(f.server(), &f.uri, cursor).await;
    let token = position_of(
        TYPE_ONLY_IMPORT_SOURCE,
        "import type { Widget }",
        "import type { ".len(),
    );
    let (_, token_offset) = provider_query_anchor(f.server(), &f.uri, token).await;
    provider.set_rename_locations(
        &tsx_path,
        query_offset,
        vec![crate::type_provider::protocol::RenameLocation {
            path: tsx_path.clone(),
            start: token_offset,
            end: token_offset + "Widget".len() as u32,
        }],
    );

    let edit = rename_at(f.server(), &f.uri, cursor, "Renamed")
        .await
        .expect("with nothing dropped, the provider's own rename must still serve");
    assert!(
        !edit_starts(&edit, &f.uri).is_empty(),
        "the provider's mapped leg must survive: {edit:?}"
    );
    f.shutdown().await;
}

// ── A Svelte carrier's markup occurrences are in NO inventory ───────────────
//
// `TemplateAnalysisSnapshot::binding_occurrences` is Verter's only same-file
// markup occurrence inventory and it is a VUE template-analysis product; a Svelte
// carrier carries no template element IR at all. So Verter's same-file rename
// proof for a `.svelte` file holds only SCRIPT spans, and every markup occurrence
// of the renamed binding is invisible to it.
//
// That is a known gap whose entire prior justification was that it fails CLOSED —
// Verter simply never claims a markup occurrence. Delegating a dropped
// current-companion leg to a gate with that inventory would VOID that property:
// the markup occurrence would be neither edited nor proven, and the partial would
// ship as a success.

/// A Svelte carrier whose markup READS the store (`$count`). The IDE projector
/// rewrites `$count` into an unmapped `__verter_store_get(count)` call, so the
/// authored `$count` bytes have no 1:1 provider correlate.
const SVELTE_STORE_SOURCE: &str = "<script lang=\"ts\">\nimport { writable } from \"svelte/store\"\nconst count = writable(0)\nconsole.log(count)\n</script>\n\n{#if true}{@const doubled = $count}{/if}\n";

/// THE PREMISE, pinned — including the TRAP in it.
///
/// A Svelte carrier's analysis DOES carry a template snapshot; what it does not
/// carry is any modelled markup occurrence. Both inventories stay empty however
/// much the markup references the binding. So the presence of a template snapshot
/// is NOT a completeness witness — an `is_some()` test would vouch for markup that
/// was never modelled — and the witness has to be the carrier's capability
/// instead.
///
/// If this ever stops holding because the markup inventory landed, this lane fails
/// and the capability should be granted for that carrier deliberately, rather than
/// the exemption silently widening.
#[tokio::test]
async fn a_svelte_carrier_models_no_markup_occurrence_despite_having_a_template_snapshot() {
    let f = fixture_for_carrier(
        SVELTE_STORE_SOURCE,
        None,
        crate::TypeProviderKind::Tsgo,
        "App.svelte",
        "svelte",
    )
    .await;
    let analysis = f
        .server()
        .documents
        .get_analysis(&f.uri)
        .expect("the Svelte carrier analyses");

    assert!(
        analysis.bindings.iter().any(|b| b.name == "count"),
        "precondition: the SCRIPT binding is analysed — only the markup half is missing: {:?}",
        analysis
            .bindings
            .iter()
            .map(|b| &b.name)
            .collect::<Vec<_>>()
    );
    assert!(
        SVELTE_STORE_SOURCE.contains("$count"),
        "precondition: the markup really does reference the store"
    );
    let template = analysis.template.as_ref().expect(
        "THE TRAP: a Svelte carrier HAS a template snapshot, so `is_some()` proves nothing",
    );
    assert!(
        template.binding_occurrences.is_empty() && template.unresolved_bindings.is_empty(),
        "a Svelte carrier models NO markup occurrence, so the markup `$count` is in no \
         inventory; got {:?} / {:?}",
        template.binding_occurrences,
        template.unresolved_bindings
    );
    f.shutdown().await;
}

/// A Svelte carrier's rename must refuse a dropped current-companion leg.
///
/// The provider reports the script occurrence (mapped) plus the markup read's
/// projection, which lives in the inserted `__verter_store_get(count)` call and
/// maps to no authored byte. Verter's proof holds only the script spans, so the
/// same-file gate — the authority the exemption delegates to — never looks at the
/// authored `$count`. Shipping the remainder renames the declaration and leaves
/// `$count` dangling.
#[tokio::test]
async fn svelte_rename_refuses_a_dropped_companion_leg_over_its_unenumerated_markup() {
    let provider = Arc::new(crate::type_provider::mock::MockTypeProvider::new());
    let f = fixture_for_carrier(
        SVELTE_STORE_SOURCE,
        Some(provider.clone()),
        crate::TypeProviderKind::None,
        "App.svelte",
        "svelte",
    )
    .await;
    let cursor = position_of(
        SVELTE_STORE_SOURCE,
        "const count = writable",
        "const co".len(),
    );
    let (tsx_path, query_offset) = provider_query_anchor(f.server(), &f.uri, cursor).await;
    let declaration = position_of(
        SVELTE_STORE_SOURCE,
        "const count = writable",
        "const ".len(),
    );
    let (_, declaration_offset) = provider_query_anchor(f.server(), &f.uri, declaration).await;
    // The markup read's projection: the argument of the INSERTED store-get call,
    // whose bytes the source map bridges to nothing.
    let (_, store_get_offset) =
        companion_offset_of(f.server(), &f.uri, "__verter_store_get(", 0).await;

    provider.set_rename_locations(
        &tsx_path,
        query_offset,
        vec![
            crate::type_provider::protocol::RenameLocation {
                path: tsx_path.clone(),
                start: declaration_offset,
                end: declaration_offset + "count".len() as u32,
            },
            crate::type_provider::protocol::RenameLocation {
                path: tsx_path.clone(),
                start: store_get_offset,
                end: store_get_offset + "__verter_store_get(".len() as u32,
            },
        ],
    );

    let edit = rename_at(f.server(), &f.uri, cursor, "renamed").await;

    assert!(
        edit.is_none(),
        "a Svelte carrier's markup occurrences are in no inventory, so a dropped companion leg \
         is behind NO gate and the whole rename must fail closed; got edits at {:?} (the authored \
         `$count` in the markup would keep the old name)",
        edit.as_ref().map(|e| edit_starts(e, &f.uri))
    );
    f.shutdown().await;
}

// ── The document/host version fence ────────────────────────────────────────
//
// A rename classifies the cursor against THREE ingredients: the authored
// source, its line index, and the analysis. Those must all describe ONE
// document version, or version-A offsets get resolved against version-B spans.
//
// `DocumentRegistry::did_change` commits an edit in two steps that are NOT
// atomic with each other: it first drops this canonical's semantic snapshot and
// upserts the HOST, and only after re-compiling the IDE surface writes the new
// source + line index into `DocumentState`. In between, `get_analysis` has no
// validated snapshot to serve and falls through to the host — which is ALREADY
// at the new version while `doc.source` / `doc.line_index` still describe the
// old one. These lanes occupy that state deterministically.

/// [`PLAIN_SCRIPT_SOURCE`] with one extra leading script line, so every authored
/// offset after it shifts by that line's byte length — more than the 10 bytes
/// between the `:title` and interpolation spellings of `count`, so no version-B
/// template span still covers a version-A cursor inside one.
const SHIFTED_PLAIN_SCRIPT_SOURCE: &str = "<script lang=\"ts\">\n// a new first line\nconst count = 1\nconsole.log(count)\n</script>\n\n<template>\n<div :title=\"count\">{{ count }}</div>\n</template>\n";

/// Move the HOST to `source` WITHOUT touching `DocumentState` — the exact
/// intermediate state a `did_change` occupies between its host upsert and its
/// registry write.
fn upsert_host_only(server: &VerterLanguageServer, uri: &Uri, source: &str) -> String {
    let canonical_id = server
        .documents
        .get_canonical_id(uri)
        .expect("the document is open");
    let file_language = verter_session::LanguageRegistry::global()
        .carrier_for_editor_language_id("vue")
        .expect("`vue` is a registered carrier language");
    let _update = server
        .documents
        .host()
        .upsert(verter_session::UpsertRequest {
            canonical_id: Some(canonical_id.clone()),
            input_id: canonical_id.clone(),
            source: Arc::from(source),
            file_language,
            aliases: Vec::new(),
        })
        .expect("host upsert succeeds");
    canonical_id
}

/// A rename must never classify its cursor against an analysis that describes a
/// DIFFERENT version of the file than the source and line index it measures
/// offsets with.
///
/// With the host one version ahead, the version-A cursor no longer lands inside
/// any version-B `unresolved_bindings` span, so the instance-member position is
/// reclassified as a NATIVE binding: the emitted transaction renames the script
/// `const`'s occurrences and leaves the requested instance member untouched —
/// the ORIGINAL defect of this surface, reached through a torn read instead of a
/// wrong predicate. Worse, version-B's declaration span is converted through
/// version-A's line index, which lands an edit in the MIDDLE of
/// `console.log(count)`. The same-file completeness gate cannot catch either,
/// because it proves the emitted set against that same torn expectation.
#[tokio::test]
async fn rename_refuses_when_the_host_analysis_is_ahead_of_the_open_document() {
    let f = fixture(PLAIN_SCRIPT_SOURCE).await;
    let position = position_of(PLAIN_SCRIPT_SOURCE, "{{ count }}", "{{ co".len());

    // Precondition 1 — coherent state, cached semantic snapshot: refused.
    let coherent = rename_at(f.server(), &f.uri, position, "renamed").await;
    assert!(
        coherent.is_none(),
        "precondition: the coherent instance-member position refuses, got {coherent:?}"
    );

    // Precondition 2 — drop the validated snapshot exactly as `did_change` does
    // (`semantic_snapshots.remove` for this canonical), so `get_analysis` is
    // served by the HOST. The answer must not change: this isolates the tear
    // below as the cause, and proves the host analysis really does carry the
    // `unresolved_bindings` span (otherwise the position would already
    // misclassify and the lane would be red for the wrong reason).
    f.server().documents.set_semantic_analysis_enabled(false);
    let host_served = rename_at(f.server(), &f.uri, position, "renamed").await;
    assert!(
        host_served.is_none(),
        "precondition: the host-served analysis for the SAME source still refuses, got \
         {host_served:?}"
    );

    // The tear: host at version B, `DocumentState` still at version A.
    let canonical_id = upsert_host_only(f.server(), &f.uri, SHIFTED_PLAIN_SCRIPT_SOURCE);
    let document_source = f
        .server()
        .documents
        .get(&f.uri)
        .map(|doc| doc.source.to_string())
        .expect("the document is open");
    let host_source = f
        .server()
        .documents
        .host()
        .get_source(&canonical_id)
        .expect("the host holds a source for the open canonical");
    assert_eq!(
        document_source, PLAIN_SCRIPT_SOURCE,
        "precondition: the registry must still hold version A"
    );
    assert_eq!(
        &*host_source, SHIFTED_PLAIN_SCRIPT_SOURCE,
        "precondition: the host must be one version ahead — without that the lane proves nothing"
    );

    let edit = rename_at(f.server(), &f.uri, position, "renamed").await;

    assert!(
        edit.is_none(),
        "a torn (source, line index, analysis) triple must emit NO WorkspaceEdit — the script \
         `const` is a DIFFERENT symbol and version-B spans do not describe version-A bytes; got \
         edits at {:?}",
        edit.as_ref().map(|e| edit_starts(e, &f.uri))
    );
    f.shutdown().await;
}

/// The same fence at a NATIVE anchor: a `<script setup>` binding is Verter's own
/// symbol and renames normally in a coherent state, but with the host one
/// version ahead its proven occurrence set is measured from mismatched spans, so
/// the rename must refuse rather than emit ranges that describe neither version.
#[tokio::test]
async fn native_rename_refuses_when_the_host_analysis_is_ahead_of_the_open_document() {
    let f = fixture(SCRIPT_SETUP_SOURCE).await;
    let position = position_of(SCRIPT_SETUP_SOURCE, "const count", "const co".len());

    let mut expected: Vec<(u32, u32)> = SCRIPT_OCCURRENCES.to_vec();
    expected.extend(TEMPLATE_OCCURRENCES);
    expected.sort_unstable();
    let coherent = rename_at(f.server(), &f.uri, position, "renamed")
        .await
        .expect("precondition: a coherent `<script setup>` binding renames");
    assert_eq!(
        edit_starts(&coherent, &f.uri),
        expected,
        "precondition: the coherent rename spans script and template"
    );

    f.server().documents.set_semantic_analysis_enabled(false);
    const SHIFTED_SCRIPT_SETUP_SOURCE: &str = "<script setup lang=\"ts\">\n// a new first line\nconst count = 1\nconsole.log(count)\n</script>\n\n<template>\n<div :title=\"count\">{{ count }}</div>\n</template>\n";
    let _ = upsert_host_only(f.server(), &f.uri, SHIFTED_SCRIPT_SETUP_SOURCE);

    let edit = rename_at(f.server(), &f.uri, position, "renamed").await;

    assert!(
        edit.is_none(),
        "a torn triple must emit NO WorkspaceEdit even for a natively-owned binding; got edits \
         at {:?}",
        edit.as_ref().map(|e| edit_starts(e, &f.uri))
    );
    f.shutdown().await;
}

/// The prepare half of the same fence. Prepare and rename must agree about who
/// owns the cursor — that is the whole reason they share one resolution — so a
/// torn triple that makes rename refuse must make prepare decline too. Otherwise
/// the client opens a rename box over a range the follow-up transaction will
/// silently decline to deliver.
#[tokio::test]
async fn prepare_rename_refuses_when_the_host_analysis_is_ahead_of_the_open_document() {
    let f = fixture(SCRIPT_SETUP_SOURCE).await;
    let position = position_of(SCRIPT_SETUP_SOURCE, "const count", "const co".len());

    let coherent = prepare_rename_at(f.server(), &f.uri, position).await;
    assert!(
        coherent.is_some(),
        "precondition: a coherent `<script setup>` binding is offered, got {coherent:?}"
    );

    f.server().documents.set_semantic_analysis_enabled(false);
    const SHIFTED_SCRIPT_SETUP_SOURCE: &str = "<script setup lang=\"ts\">\n// a new first line\nconst count = 1\nconsole.log(count)\n</script>\n\n<template>\n<div :title=\"count\">{{ count }}</div>\n</template>\n";
    let _ = upsert_host_only(f.server(), &f.uri, SHIFTED_SCRIPT_SETUP_SOURCE);

    let prepared = prepare_rename_at(f.server(), &f.uri, position).await;

    assert!(
        prepared.is_none(),
        "a torn triple must offer NO rename range, got {prepared:?}"
    );
    f.shutdown().await;
}

/// A [`crate::TypeProvider`] that behaves exactly like [`MockTypeProvider`]
/// except that `get_rename_locations` FAILS — the transport/engine-error arm of
/// the prepare matrix. Everything else delegates, so the carrier surfaces sync
/// exactly as in the other provider lanes.
struct RenameErrorProvider {
    inner: Arc<crate::type_provider::mock::MockTypeProvider>,
}

impl RenameErrorProvider {
    /// How many rename queries reached this provider (so a lane can prove the
    /// error arm was actually exercised).
    fn rename_queries(&self) -> usize {
        self.inner
            .calls()
            .iter()
            .filter(|call| {
                matches!(
                    call,
                    crate::type_provider::mock::MockCall::GetRenameLocations { .. }
                )
            })
            .count()
    }
}

impl crate::TypeProvider for RenameErrorProvider {
    fn provider_id(&self) -> &'static str {
        self.inner.provider_id()
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> crate::type_provider::traits::ProviderFuture<
        '_,
        Vec<crate::type_provider::protocol::RenameLocation>,
    > {
        // Record the query through the inner mock, then fail.
        let recorded = self.inner.get_rename_locations(path, offset);
        Box::pin(async move {
            let _ = recorded.await;
            Err(crate::type_provider::protocol::TypeProviderError::new(
                "scripted rename transport failure".to_string(),
            ))
        })
    }

    fn open_file(
        &self,
        path: &str,
        content: &str,
    ) -> crate::type_provider::traits::ProviderFuture<'_, ()> {
        self.inner.open_file(path, content)
    }

    fn load_file(
        &self,
        path: &str,
        content: &str,
    ) -> crate::type_provider::traits::ProviderFuture<'_, ()> {
        self.inner.load_file(path, content)
    }

    fn update_file(
        &self,
        path: &str,
        content: &str,
    ) -> crate::type_provider::traits::ProviderFuture<'_, ()> {
        self.inner.update_file(path, content)
    }

    fn close_file(&self, path: &str) -> crate::type_provider::traits::ProviderFuture<'_, ()> {
        self.inner.close_file(path)
    }

    fn get_completions(
        &self,
        path: &str,
        offset: u32,
        trigger_character: Option<&str>,
    ) -> crate::type_provider::traits::ProviderFuture<
        '_,
        crate::type_provider::protocol::CompletionResult,
    > {
        self.inner.get_completions(path, offset, trigger_character)
    }

    fn get_hover(
        &self,
        path: &str,
        offset: u32,
    ) -> crate::type_provider::traits::ProviderFuture<
        '_,
        Option<crate::type_provider::protocol::HoverInfo>,
    > {
        self.inner.get_hover(path, offset)
    }

    fn get_diagnostics(
        &self,
        path: &str,
    ) -> crate::type_provider::traits::ProviderFuture<
        '_,
        Vec<crate::type_provider::protocol::TypeDiagnostic>,
    > {
        self.inner.get_diagnostics(path)
    }

    fn get_definition(
        &self,
        path: &str,
        offset: u32,
    ) -> crate::type_provider::traits::ProviderFuture<
        '_,
        Vec<crate::type_provider::protocol::TypeLocation>,
    > {
        self.inner.get_definition(path, offset)
    }

    fn get_type_definition(
        &self,
        path: &str,
        offset: u32,
    ) -> crate::type_provider::traits::ProviderFuture<
        '_,
        Vec<crate::type_provider::protocol::TypeLocation>,
    > {
        self.inner.get_type_definition(path, offset)
    }

    fn get_references(
        &self,
        path: &str,
        offset: u32,
    ) -> crate::type_provider::traits::ProviderFuture<
        '_,
        Vec<crate::type_provider::protocol::TypeLocation>,
    > {
        self.inner.get_references(path, offset)
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> crate::type_provider::traits::ProviderFuture<
        '_,
        Option<crate::type_provider::protocol::SignatureHelp>,
    > {
        self.inner.get_signature_help(path, offset)
    }

    fn get_code_actions(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        diagnostics: &[crate::type_provider::protocol::ProviderDiagnosticContext],
    ) -> crate::type_provider::traits::ProviderFuture<
        '_,
        Vec<crate::type_provider::protocol::TypeCodeAction>,
    > {
        self.inner
            .get_code_actions(path, start_offset, end_offset, diagnostics)
    }

    fn get_semantic_tokens(
        &self,
        path: &str,
    ) -> crate::type_provider::traits::ProviderFuture<
        '_,
        Vec<crate::type_provider::protocol::SemanticToken>,
    > {
        self.inner.get_semantic_tokens(path)
    }

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> crate::type_provider::traits::ProviderFuture<
        '_,
        Vec<crate::type_provider::protocol::TypeDocumentHighlight>,
    > {
        self.inner.get_document_highlights(path, offset)
    }

    fn get_inlay_hints(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> crate::type_provider::traits::ProviderFuture<
        '_,
        Vec<crate::type_provider::protocol::InlayHint>,
    > {
        self.inner.get_inlay_hints(path, start_offset, end_offset)
    }
}
