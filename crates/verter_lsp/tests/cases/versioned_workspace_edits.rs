//! Edit-bearing responses through the real `verter-lsp` binary over stdio.
//!
//! A client that advertises `workspace.workspaceEdit.documentChanges` receives
//! every rename and code-action edit as a `TextDocumentEdit` naming the version
//! of the open document the request was answered against, so the client
//! rejects an edit to a document that has moved since; a client that does not
//! advertise it keeps the unversioned shapes it can apply.
//!
//! Hermetic: the binary runs with `--type-provider=off`, so rename and organize
//! imports are answered by Verter's own analysis of the open buffer.

use std::process::{Command, Stdio};

use serde_json::{json, Value};
use verter_editor_client::build_server_args;

use super::stdio_launch_smoke::{
    kill_child, next_message, path_to_file_uri, spawn_reader, write_message,
};

const APP: &str = "<script setup lang=\"ts\">\nimport { ref, computed } from 'vue'\nconst count = ref(1)\n</script>\n<template><div>{{ count }}</div></template>\n";
/// The version the client opens the document at — distinct from the `1`
/// every default would produce, so a versioned edit can only carry it by
/// reading the request's snapshot.
const OPEN_VERSION: i32 = 3;

struct Replies {
    rename: Value,
    code_actions: Value,
    uri: String,
}

/// Open `APP` in a fresh workspace and send one rename and one organize-imports
/// code action, as a client advertising `document_changes`.
fn exchange(document_changes: bool) -> Replies {
    let tmp = tempfile::tempdir().expect("temp workspace root");
    let root = tmp.path().to_string_lossy().into_owned();
    std::fs::write(tmp.path().join("tsconfig.json"), "{}").expect("write tsconfig");
    std::fs::write(tmp.path().join("App.vue"), APP).expect("write App.vue");
    let root_uri = path_to_file_uri(&root);
    let uri = path_to_file_uri(&tmp.path().join("App.vue").to_string_lossy());

    let args = build_server_args(Some(&root), &json!({ "typeProvider": "off" }));
    let mut child = Command::new(verter_test_support::cargo_test_binary_path!("verter-lsp"))
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the verter-lsp binary");
    let mut stdin = child.stdin.take().expect("child stdin piped");
    let rx = spawn_reader(child.stdout.take().expect("child stdout piped"));

    let request = |stdin: &mut std::process::ChildStdin,
                   child: &mut std::process::Child,
                   id: i64,
                   method: &str,
                   params: Value|
     -> Value {
        write_message(
            stdin,
            &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        );
        for _ in 0..256 {
            let message = next_message(&rx, child, method);
            // Answer the server's own requests so it never waits on the client.
            if let (Some(server_id), Some(_)) = (message.get("id"), message.get("method")) {
                write_message(
                    stdin,
                    &json!({ "jsonrpc": "2.0", "id": server_id, "result": null }),
                );
                continue;
            }
            if message.get("id").and_then(Value::as_i64) == Some(id) {
                if let Some(error) = message.get("error") {
                    kill_child(child);
                    panic!("{method} returned an error: {error}");
                }
                return message.get("result").cloned().unwrap_or(Value::Null);
            }
        }
        kill_child(child);
        panic!("{method}: no response");
    };

    request(
        &mut stdin,
        &mut child,
        1,
        "initialize",
        json!({
            "processId": null,
            "rootUri": root_uri,
            "workspaceFolders": [ { "uri": root_uri, "name": "versioned-edits" } ],
            "capabilities": {
                "workspace": { "workspaceEdit": { "documentChanges": document_changes } }
            },
        }),
    );
    write_message(
        &mut stdin,
        &json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
    );
    write_message(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": { "textDocument": {
                "uri": uri, "languageId": "vue", "version": OPEN_VERSION, "text": APP
            } }
        }),
    );

    // `count` in `const count = ref(1)`.
    let rename = request(
        &mut stdin,
        &mut child,
        2,
        "textDocument/rename",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 2, "character": 7 },
            "newName": "total",
        }),
    );
    let code_actions = request(
        &mut stdin,
        &mut child,
        3,
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 0 }
            },
            "context": { "diagnostics": [], "only": ["source.organizeImports"] },
        }),
    );

    request(&mut stdin, &mut child, 4, "shutdown", Value::Null);
    write_message(
        &mut stdin,
        &json!({ "jsonrpc": "2.0", "method": "exit", "params": null }),
    );
    drop(stdin);
    kill_child(&mut child);
    Replies {
        rename,
        code_actions,
        uri,
    }
}

/// The `(uri, version)` of every `TextDocumentEdit` in `edit`.
fn document_edit_versions(edit: &Value) -> Vec<(String, Value)> {
    edit.get("documentChanges")
        .and_then(Value::as_array)
        .map(|changes| {
            changes
                .iter()
                .filter_map(|change| change.get("textDocument"))
                .map(|document| {
                    (
                        document
                            .get("uri")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        document.get("version").cloned().unwrap_or(Value::Null),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The organize-imports action's edit.
fn organize_imports_edit(code_actions: &Value) -> Value {
    code_actions
        .as_array()
        .and_then(|actions| {
            actions.iter().find(|action| {
                action.get("kind").and_then(Value::as_str) == Some("source.organizeImports")
            })
        })
        .and_then(|action| action.get("edit"))
        .cloned()
        .unwrap_or_else(|| panic!("an organize-imports action is offered: {code_actions}"))
}

fn is_absent(value: Option<&Value>) -> bool {
    value.is_none_or(Value::is_null)
}

#[test]
fn a_document_changes_client_receives_versioned_edits() {
    let replies = exchange(true);

    assert!(
        is_absent(replies.rename.get("changes")),
        "a documentChanges client never receives unversioned rename `changes`: {}",
        replies.rename
    );
    assert_eq!(
        document_edit_versions(&replies.rename),
        vec![(replies.uri.clone(), json!(OPEN_VERSION))],
        "the rename names the version the request was answered against: {}",
        replies.rename
    );

    let edit = organize_imports_edit(&replies.code_actions);
    assert_eq!(
        document_edit_versions(&edit),
        vec![(replies.uri.clone(), json!(OPEN_VERSION))],
        "the code action names the version the request was answered against: {edit}"
    );
}

#[test]
fn a_client_without_document_changes_keeps_unversioned_edits() {
    let replies = exchange(false);

    let changes = replies
        .rename
        .get("changes")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("the rename keeps its `changes` shape: {}", replies.rename));
    assert!(
        changes.contains_key(&replies.uri),
        "the rename edits the open document: {}",
        replies.rename
    );
    assert!(
        is_absent(replies.rename.get("documentChanges")),
        "a client without documentChanges is not sent documentChanges for a rename: {}",
        replies.rename
    );

    let edit = organize_imports_edit(&replies.code_actions);
    assert!(
        document_edit_versions(&edit)
            .iter()
            .all(|(_, version)| version.is_null()),
        "a client without documentChanges is never handed a version: {edit}"
    );
}
