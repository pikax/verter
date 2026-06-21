//! Targeted contents-cache snapshots for edit-producing provider responses.
//!
//! An edit-producing response (code action, combined "fix all", rename) carries
//! byte ranges that the parsers convert against the content of the file each edit
//! TARGETS. The parsers run a blocking disk fallback on a cache miss, so the
//! contents-cache lock is released before parsing. Rather than cloning the whole
//! `HashMap<String, Arc<str>>` (an allocation bounded by the total number of
//! cached files, and a snapshot that goes stale the moment a concurrent
//! `update_file` runs), the caller pre-scans the response for its target
//! path(s), then clones ONLY those entries — taken FRESH, after the await that
//! produced the response.
//!
//! The scanners walk the typed response JSON (no string heuristics) and
//! canonicalize each path exactly as the corresponding parser does for its
//! content lookup, so a scanned key matches the parser's lookup key byte-for-byte.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::uri::file_uri_to_path;

/// Clone only the `paths` entries out of the locked contents cache into a small
/// snapshot. The values are `Arc<str>`, so each clone is a pointer bump; the map
/// is bounded by the response's target files, not the whole cache.
///
/// A path absent from the cache is simply omitted — the parser's own disk
/// fallback (run unlocked, after the lock is released) handles that miss.
pub fn targeted_contents_snapshot(
    cache: &HashMap<String, Arc<str>>,
    paths: &HashSet<String>,
) -> HashMap<String, Arc<str>> {
    paths
        .iter()
        .filter_map(|p| cache.get(p).map(|c| (p.clone(), Arc::clone(c))))
        .collect()
}

/// Canonical target paths referenced by a tsserver code-fix `changes` array
/// (shared by `getCodeFixes` items and `getCombinedCodeFix` responses). Each
/// entry's `fileName` is canonicalized exactly as `parse_tsserver_file_code_edits`
/// keys its content lookup.
pub fn tsserver_code_fix_change_target_paths(changes: &serde_json::Value) -> HashSet<String> {
    let mut paths = HashSet::new();
    if let Some(arr) = changes.as_array() {
        for change in arr {
            if let Some(file) = change.get("fileName").and_then(|v| v.as_str()) {
                paths.insert(verter_span::path::canonicalize_path(file));
            }
        }
    }
    paths
}

/// Canonical target paths referenced by a single tsserver code-fix action
/// (`getCodeFixes` item): the action's `changes` array.
pub fn tsserver_code_action_target_paths(action: &serde_json::Value) -> HashSet<String> {
    action
        .get("changes")
        .map(tsserver_code_fix_change_target_paths)
        .unwrap_or_default()
}

/// Canonical target paths referenced by a tsserver `getCombinedCodeFix`
/// response: its top-level `changes` array.
pub fn tsserver_combined_code_fix_target_paths(body: &serde_json::Value) -> HashSet<String> {
    body.get("changes")
        .map(tsserver_code_fix_change_target_paths)
        .unwrap_or_default()
}

/// Canonical target paths referenced by a tsserver `rename` response: each
/// `locs[].file`, canonicalized exactly as the rename closure keys its content
/// lookup.
pub fn tsserver_rename_target_paths(response: &serde_json::Value) -> HashSet<String> {
    let mut paths = HashSet::new();
    if let Some(groups) = response.get("locs").and_then(|v| v.as_array()) {
        for group in groups {
            if let Some(file) = group.get("file").and_then(|v| v.as_str()) {
                paths.insert(verter_span::path::canonicalize_path(file));
            }
        }
    }
    paths
}

/// Canonical target paths referenced by an LSP `WorkspaceEdit` value (the
/// `changes: { [uri]: … }` map keys plus each `documentChanges[].textDocument.uri`).
/// Each URI is converted to a canonical filesystem path exactly as
/// `parse_rename_edit` / `parse_text_edit_to_code_edit` key their content lookup.
///
/// Used for the LSP-shape responses (tgo rename's top-level workspace edit and
/// the workspace edit nested under a tgo code action's `edit`).
pub fn lsp_workspace_edit_target_paths(workspace_edit: &serde_json::Value) -> HashSet<String> {
    let mut paths = HashSet::new();
    if let Some(changes) = workspace_edit.get("changes").and_then(|v| v.as_object()) {
        for change_uri in changes.keys() {
            paths.insert(uri_to_canonical_path(change_uri));
        }
    }
    if let Some(doc_changes) = workspace_edit
        .get("documentChanges")
        .and_then(|v| v.as_array())
    {
        for dc in doc_changes {
            if let Some(uri) = dc
                .get("textDocument")
                .and_then(|td| td.get("uri"))
                .and_then(|v| v.as_str())
            {
                paths.insert(uri_to_canonical_path(uri));
            }
        }
    }
    paths
}

/// Canonical target paths referenced by a single LSP code-action item: the
/// workspace edit nested under its `edit` field.
pub fn lsp_code_action_target_paths(item: &serde_json::Value) -> HashSet<String> {
    item.get("edit")
        .map(lsp_workspace_edit_target_paths)
        .unwrap_or_default()
}

/// Mirror `tsgo::ipc::uri_to_file_path`: percent-decode + canonicalize a
/// `file://` URI into the cache-key path form.
fn uri_to_canonical_path(uri: &str) -> String {
    verter_span::path::canonicalize_path(&file_uri_to_path(uri))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_with(entries: &[(&str, &str)]) -> HashMap<String, Arc<str>> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), Arc::from(*v)))
            .collect()
    }

    #[test]
    fn targeted_snapshot_contains_only_requested_paths_not_whole_cache() {
        let cache = cache_with(&[
            ("d:/proj/a.ts", "const a = 1;\n"),
            ("d:/proj/b.ts", "const b = 2;\n"),
            ("d:/proj/c.ts", "const c = 3;\n"),
        ]);
        let mut want = HashSet::new();
        want.insert("d:/proj/b.ts".to_string());

        let snapshot = targeted_contents_snapshot(&cache, &want);
        assert_eq!(
            snapshot.len(),
            1,
            "the snapshot must hold only the requested path, never the whole cache"
        );
        assert!(snapshot.contains_key("d:/proj/b.ts"));
        assert!(
            !snapshot.contains_key("d:/proj/a.ts") && !snapshot.contains_key("d:/proj/c.ts"),
            "unreferenced cache entries must not be cloned into the snapshot"
        );
    }

    #[test]
    fn targeted_snapshot_omits_paths_absent_from_cache() {
        let cache = cache_with(&[("d:/proj/a.ts", "const a = 1;\n")]);
        let mut want = HashSet::new();
        want.insert("d:/proj/a.ts".to_string());
        want.insert("d:/proj/missing.ts".to_string());

        let snapshot = targeted_contents_snapshot(&cache, &want);
        assert_eq!(snapshot.len(), 1, "a path absent from the cache is omitted");
        assert!(snapshot.contains_key("d:/proj/a.ts"));
        assert!(!snapshot.contains_key("d:/proj/missing.ts"));
    }

    #[test]
    fn tsserver_code_action_scanner_extracts_exactly_change_file_names() {
        // Drive letter upper-cased on input → canonicalized to lower in the key.
        let action = serde_json::json!({
            "description": "Add import",
            "changes": [
                { "fileName": "D:/proj/App.tsx", "textChanges": [] },
                { "fileName": "D:/proj/utils.ts", "textChanges": [] }
            ]
        });
        let paths = tsserver_code_action_target_paths(&action);
        assert_eq!(paths.len(), 2);
        assert!(paths.contains("d:/proj/App.tsx"));
        assert!(paths.contains("d:/proj/utils.ts"));
        assert!(
            !paths.contains("D:/proj/App.tsx"),
            "the scanned key is canonicalized, matching the parser's content-lookup key"
        );
    }

    #[test]
    fn tsserver_combined_fix_scanner_extracts_top_level_change_file_names() {
        let body = serde_json::json!({
            "changes": [
                { "fileName": "d:/proj/a.ts", "textChanges": [] }
            ]
        });
        let paths = tsserver_combined_code_fix_target_paths(&body);
        let mut want = HashSet::new();
        want.insert("d:/proj/a.ts".to_string());
        assert_eq!(paths, want);
    }

    #[test]
    fn tsserver_rename_scanner_extracts_exactly_group_files() {
        let response = serde_json::json!({
            "locs": [
                { "file": "D:/proj/App.tsx", "locs": [] },
                { "file": "D:/proj/other.ts", "locs": [] }
            ]
        });
        let paths = tsserver_rename_target_paths(&response);
        assert_eq!(paths.len(), 2);
        assert!(paths.contains("d:/proj/App.tsx"));
        assert!(paths.contains("d:/proj/other.ts"));
    }

    #[test]
    fn lsp_workspace_edit_scanner_handles_changes_and_document_changes() {
        let we = serde_json::json!({
            "changes": {
                "file:///D:/proj/a.ts": []
            },
            "documentChanges": [
                { "textDocument": { "uri": "file:///D:/proj/b.ts" }, "edits": [] }
            ]
        });
        let paths = lsp_workspace_edit_target_paths(&we);
        assert_eq!(paths.len(), 2, "both the changes map and documentChanges are scanned");
        assert!(paths.contains("d:/proj/a.ts"));
        assert!(paths.contains("d:/proj/b.ts"));
    }

    #[test]
    fn lsp_code_action_scanner_reads_nested_edit() {
        let item = serde_json::json!({
            "title": "Add import",
            "edit": {
                "changes": {
                    "file:///D:/proj/App.tsx": []
                }
            }
        });
        let paths = lsp_code_action_target_paths(&item);
        let mut want = HashSet::new();
        want.insert("d:/proj/App.tsx".to_string());
        assert_eq!(paths, want);
    }

    #[test]
    fn lsp_code_action_scanner_empty_without_edit() {
        let item = serde_json::json!({ "title": "no-op" });
        assert!(lsp_code_action_target_paths(&item).is_empty());
    }
}
