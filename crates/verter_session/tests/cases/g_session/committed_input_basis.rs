//! F1 session bind: one committed InputBasis per request, frozen across
//! live workspace mutation, and a fence that refuses a foreign basis.

use std::sync::Arc;

use verter_session::request_context::RequestContext;
use verter_session::route_analysis_inputs::{
    commit_route_analysis_basis, project_route_analysis_inputs,
};
use verter_session::{
    commit_workspace_canonical, InputBasis, LoadWave, Observation, ObserveError, SnapshotFence,
    TornSnapshot,
};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceRead};

fn memory() -> MemoryWorkspace {
    MemoryWorkspace::new(MemoryOptions::default())
}

#[test]
fn request_binds_one_basis_and_refuses_a_second() {
    let ws = memory();
    ws.inject_file("/a.ts".to_string(), Arc::from("one"));
    let ctx = RequestContext::new(1, Arc::from("/a.ts"), false, None);
    let first = commit_workspace_canonical(&ws, "/a.ts");
    ctx.bind_committed_input(first).expect("first bind");
    ws.inject_file("/a.ts".to_string(), Arc::from("two"));
    let second = commit_workspace_canonical(&ws, "/a.ts");
    assert!(
        ctx.bind_committed_input(second).is_err(),
        "a second InputBasis on one request is torn snapshot authority"
    );
    let bound = ctx.committed_input().expect("bound");
    assert_eq!(
        bound.basis().observe("/a.ts").unwrap().file_content(),
        Some("one")
    );
}

#[test]
fn committed_observe_ignores_later_workspace_writes() {
    let ws = memory();
    ws.inject_file("/a.ts".to_string(), Arc::from("one"));
    let basis = commit_workspace_canonical(&ws, "/a.ts");
    ws.inject_file("/a.ts".to_string(), Arc::from("two"));
    assert_eq!(ws.read_file("/a.ts").as_deref(), Some("two"));
    assert_eq!(basis.observe("/a.ts").unwrap().file_content(), Some("one"));
}

#[test]
fn fence_refuses_publication_from_a_later_basis() {
    let first = InputBasis::commit(
        LoadWave::from_keys(["/a.ts"]),
        [Observation::file("/a.ts", "one")],
        [],
    )
    .expect("first");
    let second = InputBasis::commit(
        LoadWave::from_keys(["/a.ts"]),
        [Observation::file("/a.ts", "two")],
        [],
    )
    .expect("second");
    let fence = SnapshotFence::bind(&first);
    assert_eq!(fence.admit(&first), Ok(()));
    assert_eq!(fence.admit(&second), Err(TornSnapshot::BasisMismatch));
}

#[test]
fn route_analysis_snapshot_is_projected_from_the_committed_basis() {
    let ws = memory();
    let root = "/proj";
    ws.inject_file(
        format!("{root}/package.json"),
        Arc::from(r#"{ "dependencies": { "vue-router": "^4.2.0" } }"#),
    );
    ws.inject_file(
        format!("{root}/src/router/index.ts"),
        Arc::from("export default { routes: [] }"),
    );
    let basis = commit_route_analysis_basis(&ws, root);
    ws.inject_file(
        format!("{root}/package.json"),
        Arc::from(r#"{ "dependencies": {} }"#),
    );
    assert_eq!(
        basis
            .observe(&format!("{root}/package.json"))
            .unwrap()
            .file_content(),
        Some(r#"{ "dependencies": { "vue-router": "^4.2.0" } }"#)
    );
    assert_eq!(
        basis.observe(&format!("{root}/not-probed.ts")),
        Err(ObserveError::Unrecorded)
    );
    let later = commit_route_analysis_basis(&ws, root);
    assert_ne!(basis.id(), later.id());
    assert_eq!(
        later
            .observe(&format!("{root}/package.json"))
            .unwrap()
            .file_content(),
        Some(r#"{ "dependencies": {} }"#)
    );
    let fence = SnapshotFence::bind(&basis);
    assert_eq!(fence.admit(&later), Err(TornSnapshot::BasisMismatch));
    match basis.observe(&format!("{root}/layouts")) {
        Err(ObserveError::Negative(fact)) => {
            assert_eq!(fact.canonical(), format!("{root}/layouts"));
        }
        other => panic!("expected recorded layouts absence, got {other:?}"),
    }
    let inputs = project_route_analysis_inputs(&basis);
    assert_eq!(
        inputs.read_file(&format!("{root}/package.json")).as_deref(),
        Some(r#"{ "dependencies": { "vue-router": "^4.2.0" } }"#)
    );
}
