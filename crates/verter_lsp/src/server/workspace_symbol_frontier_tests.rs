//! The workspace-symbol frontier's carrier-import closure.
//!
//! The IDE-root predicate proves every project carrier's OWN symbols are a
//! Program root. It does not prove the project graph RESOLVES: a parent
//! carrier's IDE buffer imports `{child}.verter.ts`, so while that child API
//! companion is unopened the parent's use of a child symbol is unresolved —
//! and a project-wide `references` answer silently omits it.
//!
//! These tests drive the real `handle_references` path on the managed-tsgo
//! route with a scripted provider: the child's declaration must fail CLOSED
//! while its API companion is unopened, and must include the parent's usage
//! once the import-dependency publication has delivered it.

use std::sync::Arc;

use tower_lsp_server::ls_types::{
    Position, Range, ReferenceContext, ReferenceParams, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri,
};
use verter_session::{HostConfig, VerterHost};

use crate::type_provider::mock::MockTypeProvider;
use crate::type_provider::protocol::TypeLocation;
use crate::type_provider::traits::TypeProvider;

use super::super::{nav_features_navigation, VerterLanguageServer};

const CHILD_SOURCE: &str = "<script setup lang=\"ts\">\ndefineProps<{ title: string }>()\n</script>\n<template><div>{{ title }}</div></template>\n";
const PARENT_SOURCE: &str = "<script setup lang=\"ts\">\nimport ZChild from './ZChild.vue'\nconst parentHeading = 'x'\n</script>\n<template>\n  <ZChild :title=\"parentHeading\" />\n</template>\n";
/// The child after renaming its prop — a PUBLIC-SURFACE edit.
const CHILD_RENAMED_SOURCE: &str = "<script setup lang=\"ts\">\ndefineProps<{ titleText: string }>()\n</script>\n<template><div>{{ titleText }}</div></template>\n";
/// The parent after renaming its use of the child prop.
const PARENT_RENAMED_SOURCE: &str = "<script setup lang=\"ts\">\nimport ZChild from './ZChild.vue'\nconst parentHeading = 'x'\n</script>\n<template>\n  <ZChild :titleText=\"parentHeading\" />\n</template>\n";
/// The child after a template TEXT edit — no macro and no root-element change,
/// so the public surface (props + the root's inherited attrs) is UNCHANGED.
const CHILD_TEMPLATE_EDIT_SOURCE: &str = "<script setup lang=\"ts\">\ndefineProps<{ title: string }>()\n</script>\n<template><div>{{ title }} edited</div></template>\n";

struct FrontierFixture {
    _temp: tempfile::TempDir,
    service: tower_lsp_server::LspService<VerterLanguageServer>,
    drain: tokio::task::JoinHandle<()>,
    provider: Arc<MockTypeProvider>,
    workspace_id: String,
}

impl FrontierFixture {
    fn server(&self) -> &VerterLanguageServer {
        self.service.inner()
    }

    fn uri(&self, relative_path: &str) -> Uri {
        crate::uri::path_to_file_uri(&format!("{}/{relative_path}", self.workspace_id))
            .expect("file uri")
    }

    async fn shutdown(self) {
        self.drain.abort();
        drop(self.service);
    }
}

/// A managed-tsgo server over a real on-disk workspace whose configured project
/// MATERIALIZES both carriers — the production shape the frontier reads
/// (`membership.materialized_files`), so the expected-source set is the whole
/// project rather than only the file under the cursor.
async fn frontier_fixture() -> FrontierFixture {
    let temp = tempfile::tempdir().expect("temp dir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).expect("workspace dir");
    std::fs::write(workspace.join("tsconfig.json"), "{}").expect("write tsconfig");
    std::fs::write(workspace.join("src/ZChild.vue"), CHILD_SOURCE).expect("write child");
    std::fs::write(workspace.join("src/AParent.vue"), PARENT_SOURCE).expect("write parent");

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let vfs_access: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs_access));
    let host_for_server = Arc::clone(&host);
    let provider_for_server = Arc::clone(&type_provider);
    let (service, socket) = tower_lsp_server::LspService::new(move |client| {
        VerterLanguageServer::new(
            client,
            crate::LspConfig {
                host: Arc::clone(&host_for_server),
                type_provider: Some(Arc::clone(&provider_for_server)),
                project_sync_mode: crate::ProjectSyncMode::FullProject,
                type_provider_kind: crate::TypeProviderKind::Tsgo,
                type_provider_topology: crate::TypeProviderTopology::ManagedTsgo,
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
    install_materialized_workspace(server, &workspace_id);

    let mut semantic_ready = server.documents.subscribe_semantic_ready();
    for (relative_path, source) in [
        ("src/ZChild.vue", CHILD_SOURCE),
        ("src/AParent.vue", PARENT_SOURCE),
    ] {
        let canonical_id = format!("{workspace_id}/{relative_path}");
        let uri = crate::uri::path_to_file_uri(&canonical_id).expect("file uri");
        let _ = server.documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: source.to_string(),
        });
        server.documents.schedule_semantic_analysis(&uri);
    }
    for _ in 0..2 {
        tokio::time::timeout(std::time::Duration::from_secs(10), semantic_ready.recv())
            .await
            .expect("semantic analysis must settle")
            .expect("semantic ready channel stays open");
    }

    FrontierFixture {
        _temp: temp,
        service,
        drain,
        provider,
        workspace_id,
    }
}

/// Publish a snapshot whose ONE configured project materializes both carriers.
fn install_materialized_workspace(server: &VerterLanguageServer, root: &str) {
    let vfs_ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let root_cp = verter_workspace::CanonicalPath::new(root);
    let tsconfig = format!("{root}/tsconfig.json");
    let spec = verter_workspace::StaticMembershipSpec {
        files: Vec::new(),
        include: vec![verter_workspace::CompiledGlob::new(
            verter_workspace::NormalizedGlob::from_root_and_pattern(&root_cp, "**/*"),
        )],
        exclude: vec![verter_workspace::CompiledGlob::new(
            verter_workspace::NormalizedGlob::from_root_and_pattern(&root_cp, "node_modules/**"),
        )]
        .into(),
    };
    let materialized_files = [
        format!("{root}/src/ZChild.vue"),
        format!("{root}/src/AParent.vue"),
    ]
    .iter()
    .map(|path| verter_workspace::CanonicalPath::new(path))
    .collect();
    let projects = vec![
        verter_workspace::workspace_snapshot::OwnershipProject {
            id: verter_workspace::workspace_snapshot::ProjectId(0),
            root: root_cp.clone(),
            workspace_root: root_cp.clone(),
            payload: verter_workspace::workspace_snapshot::ProjectPayload::Configured {
                tsconfig_path: verter_workspace::CanonicalPath::new(&tsconfig),
                membership: verter_workspace::ConfiguredMembership {
                    spec,
                    materialized_files,
                },
                compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
                references: Vec::new(),
                workspace_aliases: Vec::new(),
            },
        },
        verter_workspace::workspace_snapshot::OwnershipProject {
            id: verter_workspace::workspace_snapshot::ProjectId(1),
            root: root_cp.clone(),
            workspace_root: root_cp.clone(),
            payload: verter_workspace::workspace_snapshot::ProjectPayload::Fallback {
                membership: verter_workspace::FallbackMembership {
                    root: root_cp.clone(),
                    exclude: vec![verter_workspace::CompiledGlob::new(
                        verter_workspace::NormalizedGlob::new(&format!("{root}/node_modules/**")),
                    )]
                    .into(),
                },
            },
        },
    ];
    let resolver = verter_workspace::ProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            root.to_string(),
            root.to_string(),
            Some(tsconfig.clone()),
        ),
    ]);
    let snapshot = Arc::new(verter_workspace::WorkspaceSnapshot {
        owners_memo: Default::default(),
        projects,
        resolver,
        generation: verter_workspace::workspace_snapshot::SnapshotGeneration(1),
    });
    let views = crate::workspace_state::build_lsp_views(&*vfs_ws, &snapshot, vec![]);
    vfs_ws.publish_snapshot(verter_workspace::PublishedRoot::with_ext(
        snapshot,
        Box::new(views),
    ));
    server.install_vfs_workspace(vfs_ws);
}

fn position_of(server: &VerterLanguageServer, uri: &Uri, needle: &str, delta: usize) -> Position {
    let doc = server.documents.get(uri).expect("document is open");
    let offset = doc
        .source
        .find(needle)
        .unwrap_or_else(|| panic!("needle `{needle}` should exist"))
        + delta;
    doc.line_index
        .offset_to_position(offset as u32)
        .expect("valid position")
}

async fn references_at(
    server: &VerterLanguageServer,
    uri: &Uri,
    position: Position,
) -> Option<Vec<tower_lsp_server::ls_types::Location>> {
    nav_features_navigation::handle_references(
        server,
        ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            context: ReferenceContext {
                include_declaration: false,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    )
    .await
    .expect("references request succeeds")
}

/// Seed the scripted provider with the PARENT's `:title` usage of the child
/// prop, keyed by the exact provider path + offset the child-declaration
/// request computes, and return the authored parent range the merge must
/// produce for it.
///
/// The seeded offsets index the DELIVERED provider bytes (the carrier-import
/// projection rewrites `./ZChild.vue` to its `.verter.ts` specifier and shifts
/// every later offset), so the mapped range is the real authored template span
/// rather than a plausible-looking neighbour.
async fn seed_parent_usage(
    fixture: &FrontierFixture,
    child_uri: &Uri,
    position: Position,
) -> Range {
    seed_parent_prop_usage(fixture, child_uri, position, "title").await
}

/// [`seed_parent_usage`] for an arbitrary prop name, so a test can rename the
/// prop on BOTH sides and still seed the parent's use of the NEW name.
async fn seed_parent_prop_usage(
    fixture: &FrontierFixture,
    child_uri: &Uri,
    position: Position,
    prop: &str,
) -> Range {
    let server = fixture.server();
    let ctx = server
        .repaired_type_provider_context(child_uri)
        .await
        .expect("the child's IDE surface is capturable");
    let offset = crate::type_provider::merge::carrier_position_to_tsx_offset_validated(
        &position,
        &ctx.carrier_line_index,
        &ctx.mapper,
        &ctx.tsx_line_index,
    )
    .expect("the child declaration maps into its IDE buffer");

    let parent_canonical = format!("{}/src/AParent.vue", fixture.workspace_id);
    let parent_ide_path = format!("{parent_canonical}.tsx");
    let parent_surface = server
        .documents
        .provider_surfaces()
        .current_snapshot(&parent_ide_path)
        .expect("the parent's IDE surface is recorded for the delivered bytes");
    // The JSX element the template's `<ZChild :prop="parentHeading" />`
    // lowers to — the parent's USAGE of the child prop.
    let jsx_usage = format!("{prop}={{parentHeading}}");
    let usage_start = parent_surface
        .provider_content
        .find(&jsx_usage)
        .expect("the delivered parent buffer carries the template usage")
        + jsx_usage.len()
        - "parentHeading}".len();
    fixture.provider.set_references(
        &ctx.tsx_path,
        offset,
        vec![TypeLocation {
            path: parent_ide_path,
            start: usage_start as u32,
            end: (usage_start + "parentHeading".len()) as u32,
        }],
    );

    let parent_uri = fixture.uri("src/AParent.vue");
    let authored = format!(":{prop}=\"parentHeading\"");
    let prefix = format!(":{prop}=\"");
    let authored_start = position_of(server, &parent_uri, &authored, prefix.len());
    let authored_end = position_of(
        server,
        &parent_uri,
        &authored,
        prefix.len() + "parentHeading".len(),
    );
    Range {
        start: authored_start,
        end: authored_end,
    }
}

/// The frontier must fail CLOSED for a references query on a child carrier's
/// prop declaration while the child's API companion — the surface the PARENT's
/// rewritten import resolves to — is still unopened. Both carrier IDE buffers
/// are live roots, so the IDE-root predicate alone reports ready and TSGO
/// answers with a set that OMITS the parent's usage.
#[tokio::test(flavor = "multi_thread")]
async fn references_on_child_declaration_fail_closed_until_imported_api_is_live() {
    let fixture = frontier_fixture().await;
    let server = fixture.server();
    let child_uri = fixture.uri("src/ZChild.vue");
    let parent_uri = fixture.uri("src/AParent.vue");

    // Both carriers reach the provider as IDE companions (the scanner's parent
    // pass + the user opening the child), and NOTHING opens the child API.
    server.ensure_current_file_synced(&parent_uri).await;
    server.ensure_current_file_synced(&child_uri).await;

    let child_canonical = format!("{}/src/ZChild.vue", fixture.workspace_id);
    let child_state = server
        .provider_sync_state_for_source(&child_canonical)
        .expect("the child carrier has a committed provider state");
    assert!(
        child_state.ide_background_loaded && child_state.commit_stamp.is_some(),
        "the child's IDE companion must be a committed root: {child_state:?}"
    );
    assert!(
        !child_state.api_background_loaded,
        "the interactive IDE sync must not have opened the child's API companion"
    );

    let position = position_of(server, &child_uri, "title: string", 0);
    seed_parent_usage(&fixture, &child_uri, position).await;

    let locations = references_at(server, &child_uri, position).await;
    assert!(
        locations.is_none(),
        "references must fail closed while the child's imported API companion is \
         unopened — a partial set omitting the parent's usage would escape: {locations:?}"
    );

    fixture.shutdown().await;
}

/// The other half: once the import-dependency publication has delivered the
/// child's API companion, the same query serves and INCLUDES the parent usage.
/// Without this half a predicate that always refused would look correct.
#[tokio::test(flavor = "multi_thread")]
async fn references_on_child_declaration_include_parent_usage_after_publication() {
    let fixture = frontier_fixture().await;
    let server = fixture.server();
    let child_uri = fixture.uri("src/ZChild.vue");
    let parent_uri = fixture.uri("src/AParent.vue");

    server.ensure_current_file_synced(&parent_uri).await;
    server.ensure_current_file_synced(&child_uri).await;

    // The background import-dependency publication opens the imported child's
    // API companion — the surface the parent's rewritten specifier resolves to.
    server
        .publish_import_dependencies_settled(&parent_uri)
        .await;

    let child_canonical = format!("{}/src/ZChild.vue", fixture.workspace_id);
    let child_state = server
        .provider_sync_state_for_source(&child_canonical)
        .expect("the child carrier has a committed provider state");
    assert!(
        child_state.api_background_loaded,
        "publication must have delivered the child's API companion: {child_state:?}"
    );

    let position = position_of(server, &child_uri, "title: string", 0);
    let expected_usage = seed_parent_usage(&fixture, &child_uri, position).await;

    let locations = references_at(server, &child_uri, position)
        .await
        .expect("references must serve once the import closure is delivered");
    assert!(
        locations
            .iter()
            .any(|location| location.uri == parent_uri && location.range == expected_usage),
        "the served answer must include the parent's `:title` usage range-exact \
         ({expected_usage:?}): {locations:?}"
    );

    fixture.shutdown().await;
}

/// The interactive IDE sync of an ALREADY-imported carrier syncs the IDE
/// companion only and closes no other buffer, so it must not commit a state
/// claiming the API companion is unloaded: that buffer is still open in the
/// provider, and every consumer of the committed state — the workspace-symbol
/// import closure, the publication's already-delivered skip, the open-vs-update
/// verb choice — would then read a delivered companion as missing.
#[tokio::test(flavor = "multi_thread")]
async fn opening_an_imported_carrier_keeps_its_delivered_api_companion_loaded() {
    let fixture = frontier_fixture().await;
    let server = fixture.server();
    let child_uri = fixture.uri("src/ZChild.vue");
    let parent_uri = fixture.uri("src/AParent.vue");
    let child_canonical = format!("{}/src/ZChild.vue", fixture.workspace_id);

    server.ensure_current_file_synced(&parent_uri).await;
    server
        .publish_import_dependencies_settled(&parent_uri)
        .await;
    let delivered = server
        .provider_sync_state_for_source(&child_canonical)
        .expect("publication commits the imported child's provider state");
    assert!(
        delivered.api_background_loaded,
        "precondition: publication delivered the child's API companion: {delivered:?}"
    );
    let delivered_api_path = delivered.api_path.clone();

    // The user now edits the child, so the interactive path genuinely re-syncs
    // its IDE companion (and only that companion) and re-commits its state.
    super::super::lifecycle::handle_did_change(
        server,
        tower_lsp_server::ls_types::DidChangeTextDocumentParams {
            text_document: tower_lsp_server::ls_types::VersionedTextDocumentIdentifier {
                uri: child_uri.clone(),
                version: 2,
            },
            content_changes: vec![tower_lsp_server::ls_types::TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: format!("{CHILD_SOURCE}<!-- edited -->\n"),
            }],
        },
    )
    .await;
    server.ensure_current_file_synced(&child_uri).await;

    let after_open = server
        .provider_sync_state_for_source(&child_canonical)
        .expect("the child keeps a committed provider state after the open");
    assert!(
        after_open.ide_background_loaded,
        "the interactive open must deliver the IDE companion: {after_open:?}"
    );
    assert_eq!(
        after_open.api_path, delivered_api_path,
        "the API companion path is owner-derived and unchanged by the open"
    );
    assert!(
        after_open.api_background_loaded,
        "the interactive IDE-only open must not report the still-open API \
         companion as unloaded: {after_open:?}"
    );

    fixture.shutdown().await;
}

/// Apply an editor edit and run the interactive repair, exactly as a feature
/// request would: `did_change` commits the new text, and the repair syncs the
/// carrier's IDE companion before any provider query captures a surface.
async fn edit_and_repair(fixture: &FrontierFixture, relative_path: &str, version: i32, text: &str) {
    let server = fixture.server();
    let uri = fixture.uri(relative_path);
    super::super::lifecycle::handle_did_change(
        server,
        tower_lsp_server::ls_types::DidChangeTextDocumentParams {
            text_document: tower_lsp_server::ls_types::VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version,
            },
            content_changes: vec![tower_lsp_server::ls_types::TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: text.to_string(),
            }],
        },
    )
    .await;
    server.ensure_current_file_synced(&uri).await;
}

/// LIVENESS is not CURRENCY. After a rename edit the child's API companion
/// buffer is still OPEN (so the carried loaded flag is a true statement), but it
/// still declares the OLD prop name while both repaired IDE surfaces use the new
/// one. TSGO would then resolve the parent's `titleText` use against an API
/// declaring `title`, drop the parent file, and hand back a credible incomplete
/// reference set — the same defect class as a never-opened companion, one stage
/// later.
#[tokio::test(flavor = "multi_thread")]
async fn references_fail_closed_while_the_delivered_api_is_stale_then_serve_after_republication() {
    let fixture = frontier_fixture().await;
    let server = fixture.server();
    let child_uri = fixture.uri("src/ZChild.vue");
    let parent_uri = fixture.uri("src/AParent.vue");

    // Warm `title`: both IDE roots live, the child's API companion delivered.
    server.ensure_current_file_synced(&parent_uri).await;
    server.ensure_current_file_synced(&child_uri).await;
    server
        .publish_import_dependencies_settled(&parent_uri)
        .await;

    // Rename the prop on BOTH sides and repair both IDE surfaces. Nothing has
    // re-delivered the child's API companion, so the provider still holds the
    // `title` declaration.
    edit_and_repair(&fixture, "src/ZChild.vue", 2, CHILD_RENAMED_SOURCE).await;
    edit_and_repair(&fixture, "src/AParent.vue", 2, PARENT_RENAMED_SOURCE).await;

    let position = position_of(server, &child_uri, "titleText: string", 0);
    let expected_usage = seed_parent_prop_usage(&fixture, &child_uri, position, "titleText").await;

    let stale = references_at(server, &child_uri, position).await;
    assert!(
        stale.is_none(),
        "references must fail closed while the delivered API companion still \
         declares the OLD prop name — a set omitting the parent would escape: {stale:?}"
    );

    // The publication re-delivers the child's API at the new surface.
    server
        .publish_import_dependencies_settled(&parent_uri)
        .await;

    let locations = references_at(server, &child_uri, position)
        .await
        .expect("references must serve once the API companion is republished");
    assert!(
        locations
            .iter()
            .any(|location| location.uri == parent_uri && location.range == expected_usage),
        "the served answer must include the parent's `titleText` use range-exact \
         ({expected_usage:?}): {locations:?}"
    );

    fixture.shutdown().await;
}

/// The currency demand is API-ROLE specific, not "any edit invalidates". An edit
/// that leaves the public surface byte-identical — the common template/body
/// change — keeps the delivered API companion CURRENT, so the frontier stays
/// ready immediately and the query never waits on a republication it does not
/// need.
#[tokio::test(flavor = "multi_thread")]
async fn an_api_neutral_edit_keeps_the_frontier_ready_without_republication() {
    let fixture = frontier_fixture().await;
    let server = fixture.server();
    let child_uri = fixture.uri("src/ZChild.vue");
    let parent_uri = fixture.uri("src/AParent.vue");

    server.ensure_current_file_synced(&parent_uri).await;
    server.ensure_current_file_synced(&child_uri).await;
    server
        .publish_import_dependencies_settled(&parent_uri)
        .await;

    // A template TEXT edit: `defineProps<{ title: string }>()` and the root
    // element are untouched, so the child's public DECLARATIONS are unchanged.
    edit_and_repair(&fixture, "src/ZChild.vue", 2, CHILD_TEMPLATE_EDIT_SOURCE).await;

    let position = position_of(server, &child_uri, "title: string", 0);
    let expected_usage = seed_parent_usage(&fixture, &child_uri, position).await;

    let locations = references_at(server, &child_uri, position)
        .await
        .expect("an API-neutral edit must not fail the frontier closed");
    assert!(
        locations
            .iter()
            .any(|location| location.uri == parent_uri && location.range == expected_usage),
        "the immediately-served answer must still include the parent's usage \
         ({expected_usage:?}): {locations:?}"
    );

    fixture.shutdown().await;
}
