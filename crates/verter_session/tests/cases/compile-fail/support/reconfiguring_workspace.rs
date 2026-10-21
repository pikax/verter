use std::sync::Arc;
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess, WorkspaceRead};

/// A memory-backed adapter that reconfigures the host at each import read.
/// Every attempt is externally superseded before publication; the bounded
/// fallback returns the correct empty component without admitting its work.
/// A weak host handle prevents a host/workspace ownership cycle. No sleeps,
/// thread arrival order or private test hook causes the mutation.
pub struct ReconfiguringWorkspace(
    MemoryWorkspace,
    std::sync::Mutex<std::sync::Weak<verter_session::VerterHost>>,
);

impl Default for ReconfiguringWorkspace {
    fn default() -> Self {
        Self(
            MemoryWorkspace::new(MemoryOptions::default()),
            Default::default(),
        )
    }
}

impl ReconfiguringWorkspace {
    pub fn bind(&self, host: &Arc<verter_session::VerterHost>) {
        *self.1.lock().unwrap() = Arc::downgrade(host);
    }
}

impl WorkspaceRead for ReconfiguringWorkspace {
    fn read_file(&self, id: &str) -> Option<Arc<str>> {
        self.0.read_file(id)
    }
    fn file_exists(&self, id: &str) -> bool {
        self.0.file_exists(id)
    }
    fn realpath(&self, id: &str) -> Option<String> {
        self.0.realpath(id)
    }
    fn reverse_deps_for(&self, id: &str) -> Vec<String> {
        self.0.reverse_deps_for(id)
    }
    fn forward_deps_for(&self, id: &str) -> Vec<String> {
        self.0.forward_deps_for(id)
    }
    fn dependency_snapshot(&self, id: &str) -> Option<verter_workspace::DependencySnapshotView> {
        self.0.dependency_snapshot(id)
    }
    fn content_generation(&self) -> u64 {
        self.0.content_generation()
    }
    fn resolution_fact_generation(&self) -> u64 {
        self.0.resolution_fact_generation()
    }
    fn resolve_import_outcome(
        &self,
        importer: &str,
        specifier: &str,
        context: verter_session_query::resolution::ResolutionContext,
    ) -> verter_workspace::ResolutionOutcome {
        let host = self.1.lock().unwrap().upgrade();
        if let Some(host) = host {
            host.configure_projects(Vec::new());
        }
        self.0.resolve_import_outcome(importer, specifier, context)
    }
}

impl WorkspaceAccess for ReconfiguringWorkspace {
    fn record_parsed_edges(&self, id: &str, edges: &[verter_workspace::ParsedEdge]) {
        self.0.record_parsed_edges(id, edges);
    }
    fn set_exact_resolutions(
        &self,
        id: &str,
        resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        self.0.set_exact_resolutions(id, resolutions)
    }
    fn record_parsed_edges_with_exact_resolutions(
        &self,
        id: &str,
        edges: &[verter_workspace::ParsedEdge],
        resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        self.0
            .record_parsed_edges_with_exact_resolutions(id, edges, resolutions)
    }
    fn replace_semantic_transitive(&self, id: &str, deps: std::collections::BTreeSet<String>) {
        self.0.replace_semantic_transitive(id, deps);
    }
    fn record_ambient_dependency(&self, consumer: &str, virtual_id: &str) {
        self.0.record_ambient_dependency(consumer, virtual_id);
    }
    fn set_default_resolve_extensions(&self, extensions: Vec<String>) {
        self.0.set_default_resolve_extensions(extensions);
    }
    fn configure_resolver(
        &self,
        projects: Vec<verter_session_query::resolution::IdeProjectConfig>,
    ) {
        self.0.configure_resolver(projects);
    }
    fn notify_upsert(&self, id: &str, source: Arc<str>) {
        self.0.notify_upsert(id, source);
    }
    fn notify_close(&self, id: &str) {
        self.0.notify_close(id);
    }
    fn notify_delete(&self, id: &str) {
        self.0.notify_delete(id);
    }
}
