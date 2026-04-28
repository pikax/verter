//! Phase 5a §6.7: session-side ambient global resolver.
//!
//! Bare-name fallback for symbols that aren't found in scope or in the
//! import graph. Looks the symbol up against the workspace's per-project
//! ambient lib symbol_index (A2) and produces a `ResolvedRootIdentity`
//! whose `canonical_id` is the project-scoped ambient virtual id
//! (`ambient:/<tag>/<canonical>`).
//!
//! The full session-side scheduler submission (lazy parse → analysis →
//! type lowering) is intentionally deferred to a follow-up sub-phase per
//! §0 binding amendment / brief: phase 5a stops at the symbol-resolution
//! and dep-recording infrastructure, and phase 5b's bare-name resolver is
//! the first caller. The signature below is fixed; subsequent sub-phases
//! extend it to issue the scheduler request internally.

use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;
use verter_workspace::ProjectStableKey;

use crate::VerterHost;

/// Resolve a bare-name symbol against the consumer project's ambient lib
/// registry. Returns `None` when no registered lib in the project exposes
/// `symbol`.
///
/// On a hit:
/// - records the consumer → ambient virtual_id reverse-dep edge so a
///   subsequent re-registration of the lib invalidates this consumer
///   through the standard dep-fact validators.
/// - returns a `ResolvedRootIdentity` whose `canonical_id` is the ambient
///   virtual id, ensuring the resolver fence reaches
///   [`HostFenceValidator`](crate::host_manage::HostFenceValidator)'s
///   ambient `WholeHash` arm (Phase 5a §6.6 / A8).
///
/// First production caller lands in 5b's bare-name resolver fallback.
#[allow(dead_code)]
pub(crate) fn resolve_ambient_global(
    host: &VerterHost,
    consumer_canonical: &str,
    consumer_project_stable_key: ProjectStableKey,
    symbol: &str,
) -> Option<ResolvedRootIdentity> {
    let workspace = host.workspace();
    let hit = workspace.lookup_ambient_symbol(consumer_project_stable_key, symbol)?;
    workspace.record_ambient_dependency(consumer_canonical, hit.virtual_id.as_ref());
    Some(ResolvedRootIdentity::new(hit.virtual_id.as_ref(), symbol))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use verter_workspace::{
        AmbientLibSpec, MemoryOptions, MemoryWorkspace, ProjectId, WorkspaceAccess,
    };

    use super::*;
    use crate::{HostConfig, VerterHost};

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
                compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
                references: vec![],
                membership: verter_workspace::ProjectMembership::default(),
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
        assert_eq!(r.symbol_name, "Pick");

        // Reverse-dep edge recorded so re-registration invalidates the consumer.
        let reverse = ws.reverse_deps_for(r.canonical_id.as_str());
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
}
