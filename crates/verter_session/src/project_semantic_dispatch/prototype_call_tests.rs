//! Tests for the ambient `Function.prototype.call` proof.
//!
//! Every test pins one clause of the proof contract: registry membership,
//! indexed occurrence identity, and the no-user-augmentation clause. The
//! negative cases are the discriminating ones — a userland `call` member,
//! an unregistered `Function`, or a merged-in user `call` contributor must
//! NEVER normalize.

use std::sync::Arc;

use verter_workspace::{
    AmbientLibSpec, MemoryOptions, MemoryWorkspace, ProjectId, WorkspaceAccess, WorkspaceRead,
};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    AuthoredPropertyKey, MacroOwnBodyStamp, MergeRoleStamp, ProjectionMode,
    ProjectionReductionContext, QueryResult, SemanticNodeData, SemanticQueryApi, SemanticQueryKey,
    SemanticQueryOutput, SignatureRef, SurfaceMember,
};
use crate::{HostConfig, UpsertRequest, VerterHost};

const FUNCTION_LIB: &str = r#"
interface Function {
  call(thisArg: any, ...args: any[]): any;
}
declare const fn: Function;
"#;

const CALLABLE_FUNCTION_LIB: &str = r#"
interface CallableFunction {
  call(thisArg: any, ...args: any[]): any;
}
declare const fn: CallableFunction;
"#;

const USERLAND: &str = r#"
interface F {
  call(): void;
}
declare const f: F;
"#;

fn ws_with_one_project() -> Arc<MemoryWorkspace> {
    let ws = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.set_project_graph(verter_workspace::ProjectGraph::from_configs(vec![
        verter_workspace::VfsProjectConfig {
            root: "/ws".to_string(),
            rank: verter_workspace::ProjectRank::Explicit,
            tsconfig_path: Some("/ws/tsconfig.json".to_string()),
            root_files: vec![],
            extensions: vec![".ts".into()],
            workspace_root: "/ws".to_string(),
            workspace_aliases: vec![],
            compiler_options: verter_semantic::resolver_core::IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: verter_workspace::configured_membership_match_all_under_root(
                &verter_workspace::CanonicalPath::new("/ws"),
            ),
        },
    ]));
    ws
}

fn host_with_ws(ws: Arc<MemoryWorkspace>) -> VerterHost {
    let access: Arc<dyn WorkspaceAccess> = ws;
    VerterHost::new(HostConfig::default(), access)
}

fn register_and_serve_lib(ws: &Arc<MemoryWorkspace>, host: &VerterHost, source: &str) -> Arc<str> {
    ws.register_ambient_lib(AmbientLibSpec {
        project_id: None,
        canonical_id: Arc::from("lib.es5.d.ts"),
        source: Arc::from(source),
    })
    .expect("ambient lib registration");
    let key = ws.project_stable_key(ProjectId(0)).expect("project key");
    let virtual_id = verter_workspace::ambient_virtual_canonical_id(key, "lib.es5.d.ts");
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: virtual_id.to_string(),
            source: Arc::from(source),
            file_language: crate::FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
    virtual_id
}

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: crate::FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
}

/// The resolved `call` member candidates of `declare const <value>: <name>`
/// in `canonical`: typeof the value (the interface surface), project the
/// `call` member, and read its overload set.
fn call_candidates(
    host: &VerterHost,
    dispatch: &ProjectSemanticDispatch,
    canonical: &str,
    _name: &str,
    value: &str,
) -> Arc<[SignatureRef]> {
    let env = host.host_view_env_hashes_for(canonical);
    let project_identity = host.host_view_project_identity_for(canonical).fold_u32();
    let receiver = match dispatch.execute_type_node(SemanticQueryKey::TypeOf {
        value_root: crate::semantic_query::ValueRootSlotIdentity::new(
            crate::semantic_query::ValueRootKey {
                scope: crate::semantic_query::ScopeId::file(
                    Arc::from(canonical),
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
                name: Arc::from(value),
            },
            project_identity,
            env.type_env_hash,
            env.lib_env_hash,
        ),
        context: crate::semantic_query::TypeOfContext::new(
            ProjectionReductionContext::published(ProjectionMode::Expanded),
            env.resolve_env_hash,
        ),
    }) {
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        other => panic!("typeof {value} in {canonical} errored: {other:?}"),
    };
    let member = match dispatch.execute_type_node(SemanticQueryKey::ProjectMember {
        base: receiver,
        member: Arc::from("call"),
        mode: ProjectionMode::Expanded,
    }) {
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        other => panic!("projecting the `call` member must resolve, got {other:?}"),
    };
    match crate::semantic_query::SemanticQueryApi::execute(
        dispatch,
        SemanticQueryKey::ResolveOverloadSet {
            callee: member,
            type_args: Arc::from(Vec::new().into_boxed_slice()),
            context: crate::semantic_query::OverloadSetContext {
                resolve_env_hash: env.resolve_env_hash,
            },
        },
    ) {
        QueryResult::Value(SemanticQueryOutput {
            value: crate::semantic_query::SemanticQueryValue::OverloadSet(refs),
            ..
        }) => refs,
        other => panic!("the `call` member overload set must resolve, got {other:?}"),
    }
}

/// The lib-declared `Function.call` occurrence proves: registry membership
/// + indexed occurrence identity + no augmentation.
#[test]
fn prototype_call_proves_for_registered_ambient_function() {
    let ws = ws_with_one_project();
    let host = host_with_ws(Arc::clone(&ws));
    let dispatch = ProjectSemanticDispatch::new(&host);
    let virtual_id = register_and_serve_lib(&ws, &host, FUNCTION_LIB);
    let refs = call_candidates(&host, &dispatch, &virtual_id, "Function", "fn");
    let key = ws.project_stable_key(ProjectId(0)).expect("project key");
    let proof = dispatch
        .prove_prototype_call(key, &refs)
        .unwrap_or_else(|| {
            panic!(
                "the registered ambient Function.call occurrence must prove; occurrence: {:?}",
                refs[0].occurrence,
            )
        });
    assert_eq!(
        proof.declaring_canonical, virtual_id,
        "the declaring canonical is the registered ambient lib's virtual id"
    );
}

/// `CallableFunction.call` proves through the same indexed identity.
#[test]
fn prototype_call_proves_for_registered_ambient_callable_function() {
    let ws = ws_with_one_project();
    let host = host_with_ws(Arc::clone(&ws));
    let dispatch = ProjectSemanticDispatch::new(&host);
    let virtual_id = register_and_serve_lib(&ws, &host, CALLABLE_FUNCTION_LIB);
    let refs = call_candidates(&host, &dispatch, &virtual_id, "CallableFunction", "fn");
    let key = ws.project_stable_key(ProjectId(0)).expect("project key");
    let proof = dispatch
        .prove_prototype_call(key, &refs)
        .expect("the registered ambient CallableFunction.call occurrence must prove");
    assert_eq!(proof.declaring_canonical, virtual_id);
}

/// A userland `call` member (not the ambient `Function` occurrence) NEVER
/// proves — no spelling match on the member name.
#[test]
fn prototype_call_refuses_userland_call_member() {
    let ws = ws_with_one_project();
    let host = host_with_ws(Arc::clone(&ws));
    let dispatch = ProjectSemanticDispatch::new(&host);
    upsert_ts(&host, "/ws/f.ts", USERLAND);
    let refs = call_candidates(&host, &dispatch, "/ws/f.ts", "F", "f");
    let key = ws.project_stable_key(ProjectId(0)).expect("project key");
    assert!(
        dispatch.prove_prototype_call(key, &refs).is_none(),
        "a userland `call` member must never prove"
    );
}

/// An unregistered `Function` (same interface NAME, no ambient
/// registration) fails the registry-membership clause.
#[test]
fn prototype_call_refuses_unregistered_function() {
    let ws = ws_with_one_project();
    let host = host_with_ws(Arc::clone(&ws));
    let dispatch = ProjectSemanticDispatch::new(&host);
    upsert_ts(&host, "/ws/shadow.ts", FUNCTION_LIB);
    let refs = call_candidates(&host, &dispatch, "/ws/shadow.ts", "Function", "fn");
    let key = ws.project_stable_key(ProjectId(0)).expect("project key");
    assert!(
        dispatch.prove_prototype_call(key, &refs).is_none(),
        "an unregistered `Function` must fail the ambient-registry clause"
    );
}

/// The no-augmentation clause: a `call` member group whose contributors
/// include a NON-lib origin fails; an all-lib group passes.
#[test]
fn call_members_all_declared_in_rejects_foreign_call_contributor() {
    let ws = ws_with_one_project();
    let host = host_with_ws(Arc::clone(&ws));
    let dispatch = ProjectSemanticDispatch::new(&host);
    let virtual_id = register_and_serve_lib(&ws, &host, FUNCTION_LIB);
    let graph = dispatch.graph();
    let void = graph.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::Void,
    ));
    let member = |origin: Option<&str>| SurfaceMember {
        key: AuthoredPropertyKey::string("call"),
        value: void,
        optional: false,
        readonly: false,
        method_kind: None,
        has_implementation_body: false,
        visibility: verter_type_expr::MemberVisibility::Public,
        spans: Default::default(),
        declaration_origin: origin.map(Arc::from),
        declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
        merge_role: MergeRoleStamp::NEUTRAL,
        excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
    };
    let surface_with = |members: Vec<SurfaceMember>| {
        graph.intern_node(SemanticNodeData::Object(
            crate::semantic_query::surface_view! {
                members: Arc::from(members.into_boxed_slice()),
                call_signatures: Arc::from(Vec::new().into_boxed_slice()),
                construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
                index_signatures: Arc::from(Vec::new().into_boxed_slice()),
                keyspace: None,
                has_index_signature: false,
            },
        ))
    };
    // All-lib group passes.
    let lib_only = surface_with(vec![member(Some(&virtual_id))]);
    assert!(
        dispatch.call_members_all_declared_in(lib_only, &virtual_id),
        "an all-lib `call` member group passes the no-augmentation clause"
    );
    // A foreign contributor (user augmentation) fails.
    let augmented = surface_with(vec![
        member(Some(&virtual_id)),
        member(Some("/ws/augment.ts")),
    ]);
    assert!(
        !dispatch.call_members_all_declared_in(augmented, &virtual_id),
        "a user-augmented `call` member group fails the no-augmentation clause"
    );
    // A member with NO declaration origin (ambiguous provenance) fails.
    let ambiguous = surface_with(vec![member(None)]);
    assert!(
        !dispatch.call_members_all_declared_in(ambiguous, &virtual_id),
        "ambiguous provenance fails the no-augmentation clause"
    );
    // No `call` member at all fails (nothing proved).
    let empty = surface_with(Vec::new());
    assert!(
        !dispatch.call_members_all_declared_in(empty, &virtual_id),
        "a surface without a `call` member proves nothing"
    );
}
