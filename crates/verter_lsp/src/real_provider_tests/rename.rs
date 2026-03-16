//! Rename tests ported from E2E suite.

use crate::test_harness::{canary_assert_known_limitation, real_provider_test};

real_provider_test!(
    rename_single_project,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;
        let _mycomp = session.open_fixture_file("src/MyComp.vue").await;

        if !session.wait_until_ready(&uri, "action.disabled", 7, "disabled").await {
            eprintln!("skipping: provider not warmed up");
            return;
        }

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
        let total = count_edits(&ws_edit);
        assert!(total >= 2, "rename doubled should have >= 2 edits, got: {total}");

        // R4: rename `count` to "counter" → ≥5 edits
        let pos = session.find_position(&uri, "const count = ref(0)", 6);
        let edits = session.rename_edits(&uri, pos, "counter").await;
        assert!(edits.is_some(), "rename count should return edits");
        let ws_edit = edits.unwrap();
        let total = count_edits(&ws_edit);
        assert!(total >= 5, "rename count should have >= 5 edits, got: {total}");

        // R5: rename `increment` to "inc" → ≥3 edits
        let pos = session.find_position(&uri, "function increment()", 9);
        let edits = session.rename_edits(&uri, pos, "inc").await;
        assert!(edits.is_some(), "rename increment should return edits");
        let ws_edit = edits.unwrap();
        let total = count_edits(&ws_edit);
        assert!(total >= 3, "rename increment should have >= 3 edits, got: {total}");

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

        // R9: cross-file rename foo prop → ≥2 files
        let pos = session.find_position(&uri, r#"foo="literal""#, 0);
        let edits = session.rename_edits(&uri, pos, "fooRenamed").await;
        assert!(edits.is_some(), "cross-file rename foo should return edits");
        let ws_edit = edits.unwrap();
        let file_count = count_files(&ws_edit);
        if session.is_tsgo() {
            // CANARY (TSGO): cross-file prop rename only returns edits in 1 file (the
            // consumer) instead of propagating to the child component's defineProps type.
            // tsserver handles this correctly. When TSGO gains cross-file rename, this
            // canary fires and should be promoted to a real assert.
            canary_assert_known_limitation!(
                file_count < 2,
                "TSGO cross-file rename only affects {file_count} file(s), expected >= 2"
            );
        } else {
            assert!(file_count >= 2, "cross-file rename should affect >= 2 files, got: {file_count}");
        }
    }
);

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

/// Count number of distinct files in a workspace edit.
fn count_files(ws_edit: &tower_lsp_server::ls_types::WorkspaceEdit) -> usize {
    let mut files = std::collections::HashSet::new();
    if let Some(changes) = &ws_edit.changes {
        for uri in changes.keys() {
            files.insert(uri.to_string());
        }
    }
    if let Some(tower_lsp_server::ls_types::DocumentChanges::Edits(doc_edits)) =
        &ws_edit.document_changes
    {
        for edit in doc_edits {
            files.insert(edit.text_document.uri.to_string());
        }
    }
    files.len()
}
