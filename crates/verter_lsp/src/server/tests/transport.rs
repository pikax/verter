use super::*;

#[test]
fn debug_snippet_ascii() {
    let content = "abcdefghijklmnopqrstuvwxyz0123456789";
    let (before, after) = debug_snippet(content, 10).unwrap();
    assert_eq!(before, "abcdefghij");
    assert_eq!(after.len(), 26); // 10..40 clamped to 10..36 = 26
}

#[test]
fn debug_snippet_multibyte_offset_inside_char() {
    // "否" is 3 bytes in UTF-8 (E5 90 A6). Place offset at byte 1 = middle of '否'.
    let content = "否abc";
    // byte 0..3 = '否', 3 = 'a', 4 = 'b', 5 = 'c'
    // offset 1 is inside '否' — must NOT panic, snaps to char boundary
    let (before, after) = debug_snippet(content, 1).unwrap();
    // Cursor snaps back to byte 0 (start of '否')
    assert!(before.is_empty(), "cursor snapped to start");
    assert!(after.contains('否'), "after contains the full character");
    assert!(after.contains('a'), "after contains subsequent ASCII");
}

#[test]
fn debug_snippet_multibyte_in_snippet_window() {
    // Reproduces the crash scenario: Chinese characters in JSDoc comments
    // with offset landing in the middle of a multi-byte char
    let content = "  /** 是否显示冷返 */\n  cold?: boolean";
    // '是' starts at byte 6, '否' at byte 9 (each CJK char is 3 bytes)
    // offset 8 lands inside '是' — must NOT panic
    let (before, after) = debug_snippet(content, 8).unwrap();
    // Cursor snaps to byte 6 (start of '是')
    assert!(before.ends_with(' '), "before ends at space before CJK");
    assert!(
        after.starts_with('是'),
        "after starts at snapped char boundary"
    );
    assert!(
        !before.contains('\u{FFFD}'),
        "no replacement chars in before"
    );
    assert!(!after.contains('\u{FFFD}'), "no replacement chars in after");
}

#[test]
fn debug_snippet_at_exact_char_boundary() {
    let content = "abc否def";
    // '否' is at bytes 3..6
    let (before, after) = debug_snippet(content, 3).unwrap();
    assert!(before.ends_with('c'));
    assert!(after.starts_with('否'));
}

#[test]
fn debug_snippet_out_of_bounds() {
    let content = "abc";
    assert!(debug_snippet(content, 100).is_none());
}

#[test]
fn debug_snippet_at_end() {
    let content = "abc";
    let result = debug_snippet(content, 3);
    // offset == len is valid (cursor at end)
    assert!(result.is_some());
}

#[tokio::test]
async fn svelte_render_call_name_navigates_to_snippet_name_range_exact() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", SVELTE_TS_SNIPPET_SOURCE)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_same_file_definition(
        service.inner(),
        &provider,
        &app_uri,
        ("@render rowSnippet", 8),
        "rowSnippet",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn svelte_js_render_call_name_navigates_to_snippet_name_range_exact() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_svelte_definition_server(&[("src/App.svelte", SVELTE_JS_SNIPPET_SOURCE)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.svelte");
    assert_svelte_same_file_definition(
        service.inner(),
        &provider,
        &app_uri,
        ("@render rowSnippet", 8),
        "rowSnippet",
    )
    .await;
    drain_handle.abort();
    drop(service);
}

/// The commit is a write point of its own. Every earlier fence in
/// the interactive repair guards a DELIVERY; the surface record pins a
/// generation and the commit then publishes the document's whole provider
/// state. An edit landing across the record's await must CANCEL the transaction
/// — the commit and the stale-path close never run, and the document is left in
/// the owed state a fresh edit leaves it in — rather than completing the
/// remaining legs and letting the admission gate refuse at the very end.
#[tokio::test(flavor = "multi_thread")]
async fn a_source_change_after_the_surface_record_cancels_the_remaining_legs() {
    let (service, provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();
    let canonical_id = "/workspace/src/App.vue";
    const SOURCE_B: &str = r#"<script setup lang="ts">
const msg = 'commit-window'
const extra = 42
</script>
<template><div>{{ msg }}{{ extra }}</div></template>
"#;

    // A restarted engine no longer holds the delivered bytes, so the repair
    // owes the IDE leg and runs it through to the record.
    provider.forget_applied_content();
    server.needs_ide_sync.insert(canonical_id.to_string());
    let (arrived, release) = server.pause_next_ide_sync_after_surface_record(canonical_id);
    let repair = server.ensure_current_file_synced(&uri);
    let edit = async {
        arrived.notified().await;
        let _ = server.documents.did_change(&uri, 2, SOURCE_B);
        release.notify_one();
    };
    futures_util::future::join(repair, edit).await;

    assert!(
        server.needs_ide_sync.contains(canonical_id),
        "a repair that observed its revision move must hand the IDE leg back \
         owed, exactly as a fresh edit does — never finish its commit for \
         superseded bytes"
    );
    assert!(
        server.needs_deferred_sync.contains(canonical_id),
        "the cancelled transaction's API leg is owed to the live revision's own \
         transaction; the cancellation re-arms it rather than dropping it"
    );
    assert!(
        server.capture_provider_request_surface(&uri).is_none(),
        "revision B committed but no surface for B was ever synced, so the \
         capture must still fail closed: the cancelled repair published nothing \
         for the live revision"
    );
}
