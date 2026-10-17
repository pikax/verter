//! Foreground-request unit tests.
//!
//! Extracted from an inline `#[cfg(test)] mod tests` in `foreground.rs`:
//! the fixtures stage temp files with `std::fs`, including writes that
//! appear behind the workspace index. The hermeticity guards scan whole
//! production source files for `std::fs::`, so those writes live in this
//! `*_tests.rs` file, which that walk skips. Wired back as a
//! `#[cfg(test)] #[path = "foreground_tests.rs"] mod tests;` child of
//! `foreground`, so `use super::*` resolves to its items.

use super::*;
use tower_lsp_server::ls_types::Command;

fn action(title: &str, target: &str) -> CodeActionOrCommand {
    let uri: Uri = target.parse().expect("uri");
    CodeActionOrCommand::CodeAction(CodeAction {
        title: title.to_string(),
        edit: Some(crate::features::action_utils::make_insert_edit(
            &uri,
            Default::default(),
            "x".to_string(),
        )),
        ..Default::default()
    })
}

/// An action whose edit cannot be bound is withdrawn whole; every other
/// action — and a command, which carries no edit — is still offered.
#[test]
fn an_unbindable_code_action_is_withdrawn_alone() {
    let unbound: Uri = "file:///Unbound.vue".parse().expect("uri");
    let mut actions = vec![
        action("bound", "file:///Bound.vue"),
        action("unbound", unbound.as_str()),
        CodeActionOrCommand::Command(Command {
            title: "command".to_string(),
            command: "verter.command".to_string(),
            arguments: None,
        }),
    ];

    let refused = actions.bind_each_edit(&mut |edit| {
        let Some(tower_lsp_server::ls_types::DocumentChanges::Edits(edits)) =
            &edit.document_changes
        else {
            return Ok(());
        };
        if edits.iter().any(|edit| edit.text_document.uri == unbound) {
            Err(EditRefusal::UnboundTarget(unbound.clone()))
        } else {
            Ok(())
        }
    });

    assert_eq!(refused, Ok(()));
    let titles: Vec<&str> = actions
        .iter()
        .map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => action.title.as_str(),
            CodeActionOrCommand::Command(command) => command.title.as_str(),
        })
        .collect();
    assert_eq!(titles, vec!["bound", "command"]);
}

/// A rename is one edit: an unbindable target refuses the whole answer.
#[test]
fn an_unbindable_rename_refuses_the_answer() {
    let unbound: Uri = "file:///Unbound.vue".parse().expect("uri");
    let mut edit = crate::features::action_utils::make_insert_edit(
        &unbound,
        Default::default(),
        "x".to_string(),
    );
    let refused = edit.bind_each_edit(&mut |_| Err(EditRefusal::UnboundTarget(unbound.clone())));
    assert_eq!(refused, Err(EditRefusal::UnboundTarget(unbound)));
}

use tower_lsp_server::ls_types::{
    CreateFile, CreateFileOptions, DocumentChangeOperation, DocumentChanges, OneOf,
    OptionalVersionedTextDocumentIdentifier, Position, Range, ResourceOp, TextDocumentEdit,
    TextDocumentItem, TextEdit,
};

fn workspace() -> (tempfile::TempDir, DocumentRegistry) {
    let temp = tempfile::tempdir().expect("temp workspace");
    let workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let host = Arc::new(verter_session::VerterHost::new(
        verter_session::HostConfig::default(),
        workspace,
    ));
    (temp, super::super::DocumentRegistry::new(host))
}

fn file_uri(path: &std::path::Path) -> Uri {
    let canonical = crate::test_utils::canonical_test_path(path);
    crate::uri::path_to_file_uri(&canonical).expect("file uri")
}

fn open_script(documents: &super::super::DocumentRegistry, uri: &Uri, version: i32) {
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "typescript".to_string(),
        version,
        text: "export const value = 1\n".to_string(),
    });
}

/// A creation the way extract builds one: `ignore_if_exists` is set, and
/// the new file is both created and written.
fn creation(source: &Uri, created: &Uri) -> WorkspaceEdit {
    WorkspaceEdit {
        document_changes: Some(DocumentChanges::Operations(vec![
            DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
                uri: created.clone(),
                options: Some(CreateFileOptions {
                    overwrite: Some(false),
                    ignore_if_exists: Some(true),
                }),
                annotation_id: None,
            })),
            DocumentChangeOperation::Edit(TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: created.clone(),
                    version: Some(99),
                },
                edits: vec![OneOf::Left(TextEdit {
                    range: Range::default(),
                    new_text: "<template><div/></template>\n".to_string(),
                })],
            }),
            DocumentChangeOperation::Edit(TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: source.clone(),
                    version: None,
                },
                edits: vec![OneOf::Left(TextEdit {
                    range: Range {
                        start: Position::new(0, 0),
                        end: Position::new(0, 0),
                    },
                    new_text: "import Created from './Created.vue'\n".to_string(),
                })],
            }),
        ])),
        ..Default::default()
    }
}

fn edit_version(edit: &WorkspaceEdit, uri: &Uri) -> Option<i32> {
    let Some(DocumentChanges::Operations(operations)) = &edit.document_changes else {
        panic!("expected operations, got {edit:?}");
    };
    for operation in operations {
        if let DocumentChangeOperation::Edit(document_edit) = operation {
            if document_edit.text_document.uri == *uri {
                return document_edit.text_document.version;
            }
        }
    }
    panic!("text edit for {}", uri.as_str());
}

fn action_creating(title: &str, source: &Uri, created: &Uri) -> CodeActionOrCommand {
    CodeActionOrCommand::CodeAction(CodeAction {
        title: title.to_string(),
        edit: Some(creation(source, created)),
        ..Default::default()
    })
}

/// An absent create is delivered with a null version. A create whose path
/// is already on disk or already open is withdrawn, `ignore_if_exists`
/// included. Recording the create, then the path appearing, settles
/// `ContentModified` while project authority is unchanged — the order
/// `answer_repaired_edit_foreground` uses (record, settle, then bind).
#[test]
fn a_created_path_is_delivered_only_while_it_stays_absent() {
    let (temp, documents) = workspace();
    let source_path = temp.path().join("Source.ts");
    std::fs::write(&source_path, "export const value = 1\n").expect("source");
    let absent_path = temp.path().join("Absent.vue");
    let existing_path = temp.path().join("Existing.vue");
    std::fs::write(&existing_path, "<template><div/></template>\n").expect("existing");
    let open_path = temp.path().join("Open.vue");

    let source = file_uri(&source_path);
    let absent = file_uri(&absent_path);
    let existing = file_uri(&existing_path);
    let open = file_uri(&open_path);
    let absent_id = super::super::uri_to_canonical_id(&absent);
    let existing_id = super::super::uri_to_canonical_id(&existing);
    assert!(
        documents
            .host()
            .workspace_read()
            .read_file(&absent_id)
            .is_none(),
        "precondition: the created path is not on disk"
    );
    assert!(
        documents
            .host()
            .workspace_read()
            .read_file(&existing_id)
            .is_some(),
        "precondition: the existing path is readable"
    );

    open_script(&documents, &source, 4);
    open_script(&documents, &open, 3);
    let request = ForegroundRequest::admit(
        &documents,
        ForegroundRoute::CodeAction,
        &source,
        WorkspaceEditSupport::VersionedDocumentChanges,
    );

    let mut refused = creation(&source, &existing);
    assert_eq!(
        request.bind_edits(&documents, &mut refused),
        Err(EditRefusal::UnboundTarget(existing.clone())),
        "a create of a file that already exists refuses the edit"
    );
    let mut open_edit = creation(&source, &open);
    assert_eq!(
        request.bind_edits(&documents, &mut open_edit),
        Err(EditRefusal::UnboundTarget(open.clone())),
        "a create of a file the client already has open refuses the edit"
    );

    let mut actions = vec![
        action_creating("extract absent", &source, &absent),
        action_creating("extract existing", &source, &existing),
        action_creating("extract open", &source, &open),
    ];
    assert_eq!(request.bind_edits(&documents, &mut actions), Ok(()));
    assert_eq!(actions.len(), 1, "only the absent create is still offered");
    let CodeActionOrCommand::CodeAction(delivered) = &actions[0] else {
        panic!("expected a code action");
    };
    assert_eq!(delivered.title, "extract absent");
    let edit = delivered.edit.as_ref().expect("edit");
    assert_eq!(
        edit_version(edit, &absent),
        None,
        "the created file is delivered with a null version"
    );
    assert_eq!(
        edit_version(edit, &source),
        Some(4),
        "the open source keeps the version captured at admission"
    );

    // Settlement sees the create only after it is recorded, and recording
    // happens before bind on the server. This request has not bound.
    let settlement = ForegroundRequest::admit(
        &documents,
        ForegroundRoute::CodeAction,
        &source,
        WorkspaceEditSupport::VersionedDocumentChanges,
    );
    let pending = vec![
        action_creating("extract absent", &source, &absent),
        action_creating("extract existing", &source, &existing),
        action_creating("extract open", &source, &open),
    ];
    assert!(
        settlement.absent_targets.lock().is_empty(),
        "precondition: settlement has not recorded a create"
    );
    assert!(
        matches!(settlement.settle(&documents, Some(())), Settled::Answer(_)),
        "an unrecorded create is not a settlement target"
    );
    settlement.record_absent_creates(&documents, &pending);
    {
        let recorded = settlement.absent_targets.lock();
        assert_eq!(recorded.len(), 1, "only the absent create is recorded");
        assert!(
            super::same_document_path(&recorded[0], &absent_id),
            "recorded {}, expected {absent_id}",
            recorded[0]
        );
    }
    assert!(
        matches!(settlement.settle(&documents, Some(())), Settled::Answer(_)),
        "an absent create still settles while the path is absent"
    );
    assert!(
        settlement.authority.is_current(documents.host()),
        "precondition: authority is unchanged before the path appears"
    );

    documents.appear_open_for_test(&absent, "<template><div/></template>\n");
    assert!(
        settlement.authority.is_current(documents.host()),
        "the appearance must not replace project authority"
    );
    assert!(
        matches!(
            settlement.settle(&documents, Some(())),
            Settled::ContentModified
        ),
        "a created path that appears after it is recorded and before settlement is content-modified"
    );
}

/// The same recorded create, when the path appears on disk and stays
/// closed. `read_file` is the predicate that sees it; an open-document
/// appearance is a different arm. The directory index cached the miss at
/// record time, so the appearance is published the way a watcher
/// `FileChanged` is — that is what makes the later `read_file` observe
/// the bytes.
#[test]
fn a_created_path_that_appears_on_disk_settles_content_modified() {
    let temp = tempfile::tempdir().expect("temp workspace");
    let workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let host = Arc::new(verter_session::VerterHost::new(
        verter_session::HostConfig::default(),
        workspace.clone(),
    ));
    let documents = super::super::DocumentRegistry::new(host);
    let source_path = temp.path().join("Source.ts");
    std::fs::write(&source_path, "export const value = 1\n").expect("source");
    let absent_path = temp.path().join("Absent.vue");
    let source = file_uri(&source_path);
    let absent = file_uri(&absent_path);
    let absent_id = super::super::uri_to_canonical_id(&absent);
    open_script(&documents, &source, 4);

    let settlement = ForegroundRequest::admit(
        &documents,
        ForegroundRoute::CodeAction,
        &source,
        WorkspaceEditSupport::VersionedDocumentChanges,
    );
    let pending = vec![action_creating("extract absent", &source, &absent)];
    settlement.record_absent_creates(&documents, &pending);
    assert!(
        matches!(settlement.settle(&documents, Some(())), Settled::Answer(_)),
        "the create settles while the path is absent"
    );
    assert!(
        settlement.authority.is_current(documents.host()),
        "precondition: authority is unchanged before the path appears"
    );

    std::fs::write(&absent_path, "<template><div/></template>\n").expect("appear on disk");
    workspace.apply_changes(vec![verter_workspace::WorkspaceChange::FileChanged {
        canonical_id: absent_id.clone(),
        source: None,
    }]);

    assert!(
        documents.open_uri_for_fs_path(&absent_id).is_none(),
        "the created path is on disk and still closed"
    );
    assert!(
        documents
            .host()
            .workspace_read()
            .read_file(&absent_id)
            .is_some(),
        "the workspace read sees the bytes written on disk"
    );
    assert!(
        settlement.authority.is_current(documents.host()),
        "a disk appearance must not replace project authority"
    );
    assert!(
        matches!(
            settlement.settle(&documents, Some(())),
            Settled::ContentModified
        ),
        "a created path that appears on disk after it is recorded and before settlement is content-modified"
    );
}

/// A closed target decoded from a scheduler commit is not a workspace
/// file. An alias resolves for `get_source` and the content hash, and
/// `read_file` of that alias is absent. Host-commit evidence still
/// settles, and a later commit of other bytes does not. The same absent
/// alias recorded as workspace bytes does not settle.
#[tokio::test]
async fn a_host_committed_alias_settles_without_a_workspace_file() {
    let (temp, documents) = workspace();
    let source_path = temp.path().join("Source.ts");
    std::fs::write(&source_path, "export const value = 1\n").expect("source");
    let child_path = temp.path().join("Child.ts");
    let child_text: Arc<str> = Arc::from("export const child = 1\n");
    std::fs::write(&child_path, child_text.as_ref()).expect("child");
    let source = file_uri(&source_path);
    let child_id = crate::test_utils::canonical_test_path(&child_path);
    let alias = "virtual-child";
    open_script(&documents, &source, 1);
    let _ = documents
        .host()
        .upsert(verter_session::UpsertRequest {
            canonical_id: Some(child_id.clone()),
            input_id: child_id.clone(),
            source: Arc::clone(&child_text),
            file_language: verter_session::FileLanguage::script_ts(),
            aliases: vec![alias.to_string()],
        })
        .expect("commit the child");
    assert!(
        documents.host().workspace_read().read_file(alias).is_none(),
        "the alias is not a workspace file"
    );
    assert_eq!(
        documents.host().get_source(alias).as_deref(),
        Some(child_text.as_ref())
    );
    assert!(documents
        .host()
        .registered_source_whole_hash(alias)
        .is_some());

    let request = ForegroundRequest::admit(
        &documents,
        ForegroundRoute::Definition,
        &source,
        WorkspaceEditSupport::VersionedDocumentChanges,
    );
    let recorded = Arc::clone(&request);
    recorded
        .compute(async {
            let source = ForegroundRequest::host_target_source(
                &documents,
                alias,
                Arc::clone(&child_text),
                ClosedTargetAuthority::HostCommit,
            );
            assert_eq!(source.as_deref(), Some(child_text.as_ref()));
            assert!(!request.target_incoherent.load(Ordering::Acquire));
        })
        .await;
    assert!(
        matches!(request.settle(&documents, Some(())), Settled::Answer(_)),
        "a host commit absent from the workspace still settles"
    );

    let edited: Arc<str> = Arc::from("export const child = 2\n");
    let _ = documents
        .host()
        .upsert(verter_session::UpsertRequest {
            canonical_id: Some(child_id.clone()),
            input_id: child_id,
            source: edited,
            file_language: verter_session::FileLanguage::script_ts(),
            aliases: vec![alias.to_string()],
        })
        .expect("commit other bytes");
    assert!(
        request.authority.is_current(documents.host()),
        "a content commit must not replace project authority"
    );
    assert!(
        matches!(
            request.settle(&documents, Some(())),
            Settled::ContentModified
        ),
        "other committed bytes supersede the recorded hash"
    );

    let workspace_request = ForegroundRequest::admit(
        &documents,
        ForegroundRoute::Definition,
        &source,
        WorkspaceEditSupport::VersionedDocumentChanges,
    );
    let recorded = Arc::clone(&workspace_request);
    recorded
        .compute(async {
            let _ = ForegroundRequest::host_target_source(
                &documents,
                alias,
                documents.host().get_source(alias).expect("alias source"),
                ClosedTargetAuthority::Workspace,
            );
        })
        .await;
    assert!(
        matches!(
            workspace_request.settle(&documents, Some(())),
            Settled::ContentModified
        ),
        "provider bytes for a path the workspace cannot read do not settle"
    );
}
