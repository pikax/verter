//! Rename tests ported from E2E suite.

use crate::test_harness::real_provider_test;
use tower_lsp_server::LanguageServer;

async fn assert_public_prop_rename_refused(
    session: &crate::test_harness::RealProviderTestSession,
    uri: &tower_lsp_server::ls_types::Uri,
    position: tower_lsp_server::ls_types::Position,
    new_name: &str,
) {
    let prepared = session.prepare_rename(uri, position).await;
    assert!(
        prepared.is_none(),
        "prepare must not offer a public component-prop rename"
    );
    let result = session
        .server()
        .rename(tower_lsp_server::ls_types::RenameParams {
            text_document_position: tower_lsp_server::ls_types::TextDocumentPositionParams {
                text_document: tower_lsp_server::ls_types::TextDocumentIdentifier {
                    uri: uri.clone(),
                },
                position,
            },
            new_name: new_name.to_string(),
            work_done_progress_params: Default::default(),
        })
        .await;
    let error = result.expect_err("public component-prop rename must return no WorkspaceEdit");
    assert_eq!(
        error.code,
        tower_lsp_server::jsonrpc::ErrorCode::ServerError(-32803)
    );
    assert!(
        error.message.contains("complete cross-file usage proof")
            && error.message.contains("no rename edit was produced"),
        "the refusal must explain the incomplete workspace proof, got {:?}",
        error.message
    );
}

real_provider_test!(
    rename_single_project,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;
        let mycomp = session.open_fixture_file("src/MyComp.vue").await;
        // Rename is capture-only and never joins background publication. A test
        // asserting complete cross-file edits must join the explicit sync receipts
        // before issuing it; production instead fails closed promptly when the
        // same completeness is not available yet.
        session.ensure_synced(&uri).await;
        session.ensure_synced(&mycomp).await;
        session.settle_import_dependencies(&uri).await;

        // The completion probe verifies that the engine indexed the exact active
        // snapshot after those receipts settled.
        let _warm = session.wait_until_ready(&uri, "action.disabled", 7, "disabled").await;

        // R1: prepare rename on `const count = ref(0)` → accepted
        let pos = session.find_position(&uri, "const count = ref(0)", 6);
        let result = session.prepare_rename(&uri, pos).await;
        assert!(result.is_some(), "prepare rename on count should be accepted");

        // R2: prepare rename on `<h1>` → rejected
        let pos = session.find_position(&uri, "<h1>", 1);
        let result = session.prepare_rename(&uri, pos).await;
        assert!(result.is_none(), "prepare rename on HTML tag should be rejected");

        // R3: rename `doubled` to "doubledValue" → ≥2 edits
        let pos = session.find_position(&uri, "const doubled = computed(", 6);
        let edits = session.rename_edits(&uri, pos, "doubledValue").await;
        assert!(edits.is_some(), "rename doubled should return edits");
        let ws_edit = edits.unwrap();
        assert_no_duplicate_edits(&ws_edit);
        let total = count_edits(&ws_edit);
        assert!(total >= 2, "rename doubled should have >= 2 edits, got: {total}");

        // R4: rename `count` to "counter" → ≥5 edits
        let pos = session.find_position(&uri, "const count = ref(0)", 6);
        let edits = session.rename_edits(&uri, pos, "counter").await;
        assert!(edits.is_some(), "rename count should return edits");
        let ws_edit = edits.unwrap();
        assert_no_duplicate_edits(&ws_edit);
        let total = count_edits(&ws_edit);
        assert!(total >= 5, "rename count should have >= 5 edits, got: {total}");

        // R5: rename `increment` to "inc" → ≥3 edits
        let pos = session.find_position(&uri, "function increment()", 9);
        let edits = session.rename_edits(&uri, pos, "inc").await;
        assert!(edits.is_some(), "rename increment should return edits");
        let ws_edit = edits.unwrap();
        assert_no_duplicate_edits(&ws_edit);
        let total = count_edits(&ws_edit);
        assert!(total >= 3, "rename increment should have >= 3 edits, got: {total}");

        // Exact editor-contract shape: local bindings repeated in both script and
        // template produce one mutation per authored occurrence, never one static
        // edit plus a duplicate provider edit for the same range.
        let contract = session
            .open_virtual(
                "src/RenameContract.vue",
                r#"<script setup lang="ts">
import DirectChild from "./MyComp.vue";

interface ContractValue {
  label: string;
  count: number;
}

const typedValue: ContractValue = { label: "typed", count: 1 };
function renderTyped(): string {
  return `${typedValue.label}:${typedValue.count}`;
}
</script>
<template>
  <main>
    <button @click="renderTyped">{{ typedValue.label }}</button>
    <DirectChild :foo="typedValue.label" />
  </main>
</template>
"#,
            )
            .await;
        session.ensure_synced(&contract).await;
        session.settle_import_dependencies(&contract).await;
        let script_pos = session.find_position(&contract, "const typedValue", 6);
        let script_edit = session
            .rename_edits(&contract, script_pos, "typedDatum")
            .await
            .expect("script-origin contract rename returns edits");
        assert_no_duplicate_edits(&script_edit);
        assert_eq!(count_edits(&script_edit), 5, "{script_edit:?}");

        let markup_pos = session.find_position(&contract, "{{ typedValue.label }}", 3);
        let markup_edit = session
            .rename_edits(&contract, markup_pos, "typedDatum")
            .await
            .expect("markup-origin contract rename returns edits");
        assert_no_duplicate_edits(&markup_edit);
        assert_eq!(count_edits(&markup_edit), 5, "{markup_edit:?}");

        // R6: prepare rename on v-if directive → rejected
        let pos = session.find_position(&uri, r#"v-if="selectedUser""#, 2);
        let result = session.prepare_rename(&uri, pos).await;
        assert!(result.is_none(), "prepare rename on v-if should be rejected");

        // R7: prepare rename on $event → rejected
        let pos = session.find_position(&uri, "handleInput($event)", 12);
        let result = session.prepare_rename(&uri, pos).await;
        assert!(result.is_none(), "prepare rename on $event should be rejected");

        // R8: rename `handleCustom` to "onCustomEvent" → ≥2 edits
        let pos = session.find_position(&uri, "function handleCustom(", 9);
        let edits = session.rename_edits(&uri, pos, "onCustomEvent").await;
        assert!(edits.is_some(), "rename handleCustom should return edits");
        let ws_edit = edits.unwrap();
        let total = count_edits(&ws_edit);
        assert!(total >= 2, "rename handleCustom should have >= 2 edits, got: {total}");

        // R9: public component props are refused. A provider's positive
        // locations cannot prove that no other parent still uses `foo`.
        session.settle_import_dependencies(&uri).await;
        let pos = session.find_position(&uri, r#"foo="literal""#, 0);
        assert_public_prop_rename_refused(session, &uri, pos, "fooRenamed").await;
    }
);

// Public-prop refusal remains mandatory when the child is closed and its API
// surface is already provider-ready. Readiness proves positive locations only.
real_provider_test!(
    rename_cross_file_prop_child_closed_refuses,
    fixture = "single-project",
    async fn run(session) {
        // Open ONLY the parent. The child MyComp.vue is intentionally NOT opened.
        let app = session.open_fixture_file("src/App.vue").await;

        // Even with the child API prewarmed and provider-ready, no available
        // surface proves there is no unseen sibling parent.
        session.ensure_synced(&app).await;
        session.settle_import_dependencies(&app).await;
        let _warm = session.wait_until_ready(&app, "action.disabled", 7, "disabled").await;

        let pos = session.find_position(&app, r#"foo="literal""#, 0);
        assert_public_prop_rename_refused(session, &app, pos, "fooRenamed").await;
    }
);

// A kebab-case parent usage is refused for rename while the independent
// references feature keeps its existing cross-file behavior.
real_provider_test!(
    rename_kebab_prop_usage_refuses_but_references_span_script_and_template,
    fixture = "single-project",
    async fn run(session) {
        let child = session
            .open_virtual(
                "src/KebabChild.vue",
                "<script setup lang=\"ts\">\ndefineProps<{ myProp: string }>()\n</script>\n",
            )
            .await;
        let parent = session
            .open_virtual(
                "src/KebabParent.vue",
                "<script setup lang=\"ts\">\nimport KebabChild from './KebabChild.vue'\nconst v = 'x'\n</script>\n<template>\n  <KebabChild :my-prop=\"v\" />\n</template>\n",
            )
            .await;
        session.ensure_synced(&child).await;
        session.ensure_synced(&parent).await;
        session.settle_import_dependencies(&parent).await;
        let _warm = session
            .wait_until_ready(&parent, ":my-prop=", 1, "my-prop")
            .await;

        // ── Rename refusal ────────────────────────────────────────────────
        let pos = session.find_position(&parent, ":my-prop=", 1);
        assert_public_prop_rename_refused(session, &parent, pos, "myPropRenamed").await;

        // ── Find-references spanning script + template ────────────────────
        // From the kebab usage, references must discover BOTH the parent
        // template usage (the provider's own leg) AND the child script
        // declaration (Verter injects the resolved `defineProps` declaration
        // — providers do not enumerate it across the synthesized API
        // surface). The camelCase↔kebab-case mapping spans the two files.
        let mut refs = Vec::new();
        for attempt in 0..12 {
            refs = session.references(&parent, pos).await;
            let spans = refs.iter().any(|l| uris_identify_same_file(&l.uri, &parent))
                && refs.iter().any(|l| uris_identify_same_file(&l.uri, &child));
            if spans || attempt == 11 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        assert!(
            refs.iter().any(|l| uris_identify_same_file(&l.uri, &parent)),
            "references must include the parent template usage; got: {refs:?}"
        );
        let decl_ref = refs
            .iter()
            .find(|l| uris_identify_same_file(&l.uri, &child))
            .expect("references must include the child script declaration");
        assert_eq!(
            (decl_ref.range.start.line, decl_ref.range.start.character),
            (1, 14),
            "the declaration reference must land on `myProp` exactly: {decl_ref:?}"
        );
    }
);

// FAIL-CLOSED GUARD — the provider-agnostic child-declaration SYNTHESIS must NEVER
// fabricate a cross-file child edit for a rename that is NOT on a `<Child prop=…>`
// usage. Renaming a LOCAL `<script setup>` binding (`doubled`, a computed) is a
// purely in-/same-file rename: its edits stay within App.vue's own surfaces and
// must NOT spill into the imported, CLOSED MyComp.vue.
//
// DISCRIMINATES: a synthesis that fired on a non-prop position (e.g. mis-resolved
// the cursor to a child component, or located a bogus carrier range) would inject a
// spurious `MyComp.vue.ts` rename location → the merge would land a wrong edit in
// MyComp.vue. The `classify_child_prop_rename` classification returns `NotChildProp`
// for a non-prop cursor (so nothing is synthesized AND the completeness gate does not
// fire), so MyComp.vue stays untouched. Both providers run the SAME classification +
// synthesis, so this guards both.
real_provider_test!(
    rename_local_binding_does_not_synthesize_cross_file_child_edit,
    fixture = "single-project",
    async fn run(session) {
        let app = session.open_fixture_file("src/App.vue").await;
        let child_uri = session.workspace_uri("src/MyComp.vue");
        let _warm = session.wait_until_ready(&app, "action.disabled", 7, "disabled").await;

        // Rename a LOCAL computed binding (NOT a cross-file prop).
        let pos = session.find_position(&app, "const doubled = computed(", 6);
        let edits = session.rename_edits(&app, pos, "doubledValue").await;
        let ws_edit = edits.expect("renaming a local binding must return edits");

        // The rename must touch App.vue (its own usages) …
        assert!(
            edit_touches(&ws_edit, &app),
            "local rename must edit App.vue: {ws_edit:?}"
        );
        // … and must NOT fabricate any edit into the imported, CLOSED MyComp.vue.
        assert!(
            !edit_touches(&ws_edit, &child_uri),
            "local-binding rename must NOT synthesize a cross-file edit into MyComp.vue \
             (fail-closed: synthesis only fires on a `<Child prop=…>` usage): {ws_edit:?}"
        );
    }
);

// Imported-type prop declarations do not make parent-usage enumeration
// authoritative. Both providers must refuse before their rename query.
#[tokio::test(flavor = "multi_thread")]
async fn rename_cross_file_imported_prop_refuses_tsserver() {
    use crate::test_harness::{TestProviderKind, TestSessionBuilder};

    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture("single-project")
        .build()
        .await
    else {
        return;
    };

    let parent = session
        .open_fixture_file("src/ImportedPropParent.vue")
        .await;
    let _child = session.open_fixture_file("src/ImportedPropChild.vue").await;
    let _warm = session
        .wait_until_ready(&parent, "headline=\"hi there\"", 0, "headline")
        .await;

    let pos = session.find_position(&parent, "headline=\"hi there\"", 0);
    assert_public_prop_rename_refused(&session, &parent, pos, "headlineRenamed").await;

    session.shutdown().await;
}

// Direct tsgo lane for the same imported-type public-prop refusal.
#[tokio::test(flavor = "multi_thread")]
async fn rename_cross_file_imported_prop_refuses_tsgo() {
    use crate::test_harness::{TestProviderKind, TestSessionBuilder};

    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsgo)
        .fixture("single-project")
        .build()
        .await
    else {
        return;
    };

    let parent = session
        .open_fixture_file("src/ImportedPropParent.vue")
        .await;
    let _child = session.open_fixture_file("src/ImportedPropChild.vue").await;
    let _warm = session
        .wait_until_ready(&parent, "headline=\"hi there\"", 0, "headline")
        .await;

    let pos = session.find_position(&parent, "headline=\"hi there\"", 0);
    assert_public_prop_rename_refused(&session, &parent, pos, "headlineRenamed").await;

    session.shutdown().await;
}

// The provider-matrix lane makes the imported-type outcome uniform: prepare
// declines and direct rename returns the public-prop refusal for both providers.
real_provider_test!(
    rename_cross_file_imported_prop_fails_closed,
    fixture = "single-project",
    async fn run(session) {
        let parent = session.open_fixture_file("src/ImportedPropParent.vue").await;
        let _child = session.open_fixture_file("src/ImportedPropChild.vue").await;
        let _warm = session
            .wait_until_ready(&parent, "headline=\"hi there\"", 0, "headline")
            .await;

        let pos = session.find_position(&parent, "headline=\"hi there\"", 0);
        assert_public_prop_rename_refused(session, &parent, pos, "headlineRenamed").await;
    }
);

// PREWARM REGRESSION GUARD — cross-file rename now DEPENDS on the parent's
// `did_open` prewarming the imported child's `{carrier}.ts` PUBLIC-API surface
// into the `ProviderSurfaceStore` (so tsserver can REPORT the cross-file rename
// location, which the generation-pinned snapshot then maps back onto the `.vue`).
// This guard proves the post-condition directly: opening ONLY the parent App.vue
// records a `CarrierApi` snapshot for the imported child MyComp.vue. If a future
// change removes/breaks the prewarm, the snapshot is absent and this guard fails
// LOUDLY — independent of the slower end-to-end rename lanes.
//
// tsserver-only: the imported-carrier prewarm is gated `matches!(.. Tsserver)`
// (lifecycle.rs) — tsgo never records here, so this is written directly (not via
// `real_provider_test!`, which would also emit a tsgo variant) and mirrors the
// macro's skip/build gating. Prewarm is LEFT ON (no `suppress_imported_carrier_prewarm`).
//
// DISCRIMINATION: setting `suppress_imported_carrier_prewarm(true)` (the inverse
// of what this guard protects) makes the snapshot never get recorded, so the
// bounded settle loop exhausts and the assertion fails — verified during
// development.
#[tokio::test(flavor = "multi_thread")]
async fn parent_did_open_prewarms_imported_child_carrier_api() {
    use crate::test_harness::{TestProviderKind, TestSessionBuilder};

    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture("single-project")
        // Prewarm ON — this guard protects exactly that behavior.
        .build()
        .await
    else {
        return;
    };

    // Open ONLY the parent. The child MyComp.vue is intentionally NOT opened; the
    // ONLY thing that can record its API surface is the parent's did_open prewarm.
    let _app = session.open_fixture_file("src/App.vue").await;

    // The child's recorded identity: its canonical id is what `did_open` computes
    // and stores as `source_canonical`; its provider path is the carrier API
    // `{canonical}.ts` virtual path the prewarm syncs/records under.
    let child_canonical =
        crate::documents::uri_to_canonical_id(&session.workspace_uri("src/MyComp.vue"));
    let child_provider_path =
        verter_semantic::resolver_core::carrier_api_provider_path(&child_canonical);

    // The lightweight imported-carrier sync is async (a no-response provider
    // notification), so give it a BOUNDED settle — a short retry reading the store,
    // NOT a fixed long sleep. The loop exits the moment the snapshot appears.
    let store = session.server().test_documents().provider_surfaces();
    let mut snapshot = None;
    for attempt in 0..40 {
        if let Some(snap) = store.current_snapshot(&child_provider_path) {
            snapshot = Some(snap);
            break;
        }
        if attempt < 39 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    // The store must hold a CarrierApi snapshot for the imported child — recorded
    // by the parent's did_open prewarm (the child itself is closed).
    let snapshot = snapshot.expect(
        "parent App.vue did_open must prewarm the imported child MyComp.vue's \
         {carrier}.ts API surface into the ProviderSurfaceStore (no snapshot recorded)",
    );
    assert!(
        store.is_tracked(&child_provider_path),
        "the child carrier API path {child_provider_path} must be CURRENTLY tracked after prewarm"
    );
    // The snapshot must be the imported CHILD's API surface — not some unrelated
    // path. An empty-store or no-child outcome fails the `expect` above; this
    // pins the recorded snapshot to MyComp specifically.
    assert_eq!(
        &*snapshot.source_canonical,
        child_canonical.as_str(),
        "the recorded snapshot must belong to the imported child MyComp.vue"
    );
    assert_eq!(
        snapshot.kind,
        crate::provider_surface_store::ProviderSurfaceKind::CarrierApi,
        "the prewarmed surface must be the {{carrier}}.ts PUBLIC-API surface"
    );

    // The captured CarrierApi set (the in-flight pinned set a cross-file rename
    // holds) must be NON-EMPTY and contain the child — this is exactly what the
    // rename's `capture_current_carrier_api_set()` would pin.
    let captured = store.capture_current_carrier_api_set();
    assert!(
        !captured.is_empty(),
        "captured CarrierApi set must be non-empty after prewarm"
    );
    assert!(
        captured.snapshot_for(&child_provider_path).is_some(),
        "captured CarrierApi set must contain the imported child MyComp.vue's API surface"
    );

    session.shutdown().await;
}

// Refusal is independent of child prewarm/project membership, so the formerly
// blocked unprewarmed lane is now an active assertion.
#[tokio::test(flavor = "multi_thread")]
async fn rename_cross_file_prop_child_closed_unprewarmed_refuses_tsserver() {
    use crate::test_harness::{TestProviderKind, TestSessionBuilder};

    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture("single-project")
        .suppress_imported_carrier_prewarm(true)
        .build()
        .await
    else {
        return;
    };

    // Open only the parent with child prewarm suppressed. Refusal is independent
    // of provider project membership because it happens before provider rename.
    let app = session.open_fixture_file("src/App.vue").await;
    let pos = session.find_position(&app, r#"foo="literal""#, 0);
    assert_public_prop_rename_refused(&session, &app, pos, "fooRenamed").await;

    session.shutdown().await;
}

/// Whether a workspace edit contains any edit for `uri`.
fn edit_touches(
    ws_edit: &tower_lsp_server::ls_types::WorkspaceEdit,
    uri: &tower_lsp_server::ls_types::Uri,
) -> bool {
    if let Some(changes) = &ws_edit.changes {
        if changes
            .keys()
            .any(|edited_uri| uris_identify_same_file(edited_uri, uri))
        {
            return true;
        }
    }
    if let Some(tower_lsp_server::ls_types::DocumentChanges::Edits(doc_edits)) =
        &ws_edit.document_changes
    {
        return doc_edits
            .iter()
            .any(|e| uris_identify_same_file(&e.text_document.uri, uri));
    }
    false
}

/// Compare file URIs using the same filesystem-identity policy as production.
///
/// TypeScript canonicalizes filenames according to the host filesystem (for
/// example `App.vue` can be returned as `app.vue` on Windows). A direct URI-key
/// comparison therefore rejects a valid edit for the same file on
/// case-insensitive filesystems while still being required on Linux, where the
/// differently-cased paths may identify distinct files.
fn uris_identify_same_file(
    left: &tower_lsp_server::ls_types::Uri,
    right: &tower_lsp_server::ls_types::Uri,
) -> bool {
    let left_path = crate::documents::uri_to_canonical_id(left);
    let right_path = crate::documents::uri_to_canonical_id(right);
    verter_span::path::fs_paths_equal(&left_path, &right_path)
}

/// Count total text edits across all files in a workspace edit.
fn count_edits(ws_edit: &tower_lsp_server::ls_types::WorkspaceEdit) -> usize {
    let mut total = 0;
    if let Some(changes) = &ws_edit.changes {
        for edits in changes.values() {
            total += edits.len();
        }
    }
    if let Some(tower_lsp_server::ls_types::DocumentChanges::Edits(doc_edits)) =
        &ws_edit.document_changes
    {
        for edit in doc_edits {
            total += edit.edits.len();
        }
    }
    total
}

/// A rename transaction must never ask an editor to apply the same mutation
/// twice. Duplicate edits can corrupt text when a client applies them
/// sequentially even if the ranges and replacement spelling are identical.
fn assert_no_duplicate_edits(ws_edit: &tower_lsp_server::ls_types::WorkspaceEdit) {
    let mut seen = std::collections::HashSet::new();
    if let Some(changes) = &ws_edit.changes {
        for (uri, edits) in changes {
            for edit in edits {
                assert!(
                    seen.insert(format!("{uri:?}:{:?}:{:?}", edit.range, edit.new_text)),
                    "rename returned a duplicate edit: {ws_edit:?}"
                );
            }
        }
    }
    if let Some(tower_lsp_server::ls_types::DocumentChanges::Edits(doc_edits)) =
        &ws_edit.document_changes
    {
        for edit in doc_edits {
            for operation in &edit.edits {
                assert!(
                    seen.insert(format!("{:?}:{operation:?}", edit.text_document.uri)),
                    "rename returned a duplicate edit: {ws_edit:?}"
                );
            }
        }
    }
}
