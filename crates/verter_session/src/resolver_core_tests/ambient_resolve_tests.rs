use crate::resolver_core::ambient_resolve::*;
use crate::HostConfig;
use crate::VerterHost;
use std::sync::Arc;
use verter_session_query::resolution::ProjectId;
use verter_workspace::AmbientLibSpec;
use verter_workspace::MemoryOptions;
use verter_workspace::MemoryWorkspace;
use verter_workspace::WorkspaceAccess;
use verter_workspace::WorkspaceRead;

fn ws_with_one_project() -> Arc<MemoryWorkspace> {
    let ws = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.set_project_graph(verter_workspace::ProjectGraph::from_configs(vec![
        verter_workspace::VfsProjectConfig {
            root: "/ws".to_string(),
            rank: verter_workspace::ProjectRank::Explicit,
            tsconfig_path: Some("/ws/tsconfig.json".to_string()),
            root_files: vec![],
            extensions: vec![".ts".into(), ".vue".into()],
            workspace_root: "/ws".to_string(),
            workspace_aliases: vec![],
            compiler_options: verter_session_query::resolution::IdeProjectCompilerOptions::default(
            ),
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

const STUB_LIB: &str = r#"
    interface Pick<T, K extends keyof T> { /* */ }
    type Partial<T> = { [P in keyof T]?: T[P] };
"#;

#[test]
fn resolves_registered_ambient_symbol_to_virtual_canonical() {
    let ws = ws_with_one_project();
    ws.register_ambient_lib(AmbientLibSpec {
        project_id: None,
        canonical_id: Arc::from("lib.es5.d.ts"),
        source: Arc::from(STUB_LIB),
    })
    .unwrap();
    let key = ws.project_stable_key(ProjectId(0)).unwrap();
    let host = host_with_ws(Arc::clone(&ws));
    let r = resolve_ambient_global(&host, "/ws/main.ts", key, "Pick").unwrap();
    assert!(
        r.canonical_id.starts_with("ambient:/"),
        "ambient hit MUST surface the project-scoped virtual id; got {}",
        r.canonical_id
    );
    assert!(r.canonical_id.ends_with("/lib.es5.d.ts"));
    assert_eq!(r.symbol_name.as_ref(), "Pick");

    // Reverse-dep edge recorded so re-registration invalidates the consumer.
    let reverse = ws.reverse_deps_for(r.canonical_id.as_ref());
    assert!(
        reverse.iter().any(|c| c == "/ws/main.ts"),
        "consumer MUST be in the reverse-dep set of the ambient virtual id"
    );
}

#[test]
fn unknown_symbol_returns_none_without_recording_edge() {
    let ws = ws_with_one_project();
    ws.register_ambient_lib(AmbientLibSpec {
        project_id: None,
        canonical_id: Arc::from("lib.es5.d.ts"),
        source: Arc::from(STUB_LIB),
    })
    .unwrap();
    let key = ws.project_stable_key(ProjectId(0)).unwrap();
    let host = host_with_ws(Arc::clone(&ws));
    let r = resolve_ambient_global(&host, "/ws/main.ts", key, "DoesNotExist");
    assert!(r.is_none());
    // No reverse-dep edge recorded for the consumer.
    let virt = verter_workspace::ambient_virtual_canonical_id(key, "lib.es5.d.ts");
    let reverse = ws.reverse_deps_for(virt.as_ref());
    assert!(
        !reverse.iter().any(|c| c == "/ws/main.ts"),
        "edge MUST NOT be recorded on a miss"
    );
}
