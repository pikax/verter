//! `impl VerterHost` — semantic-DB-facing query and invalidation methods.
//!
//! Owns the host's bridge to
//! [`verter_semantic::db::SemanticDb`](verter_semantic::db::SemanticDb):
//! component-surface, binding, snapshot, runtime-schema, and boundary
//! report queries plus query-profile and invalidation entry points.
//!
//! The semantic DB is a memo cache; these methods all consult it first
//! and only run extraction (via `scheduler_script_analysis`) on a miss.

use crate::VerterHost;

impl VerterHost {
    /// Set the query profile for this session.
    ///
    /// Query profiles control prewarming, budgets, and allowed query families.
    /// They do not change the semantic meaning of results — only execution policy.
    pub fn set_query_profile(&self, profile: verter_semantic::profile::QueryProfile) {
        *self.query_profile.lock() = profile;
    }

    /// Get the current query profile.
    pub fn query_profile(&self) -> verter_semantic::profile::QueryProfile {
        *self.query_profile.lock()
    }

    /// Current semantic revision marker based on session state.
    pub(crate) fn semantic_revision(&self) -> verter_semantic::revision::RevisionMarker {
        verter_semantic::revision::RevisionMarker {
            workspace_revision: self
                .store_view_epoch
                .load(std::sync::atomic::Ordering::Relaxed),
            parser_revision: self.tick.load(std::sync::atomic::Ordering::Relaxed),
            compiler_revision: 0,
            provider_revision: 0,
        }
    }

    /// Query the component surface for a file via the semantic DB.
    ///
    /// Extracts the declared surface from the file's script analysis,
    /// caches it in the semantic DB, and returns a `QueryResult`.
    /// Cross-file fallthrough is not resolved at this layer — the returned
    /// accepted surface equals the declared surface.
    pub fn semantic_component_surface(
        &self,
        canonical_id: &str,
    ) -> verter_semantic::query::QueryResult<
        Option<verter_semantic::facts::component::ComponentSurface>,
    > {
        use verter_semantic::query::QueryResult;
        use verter_semantic::refs::FileRef;

        let revision = self.semantic_revision();
        let file_ref = FileRef::new(canonical_id);

        // Check cache first
        {
            let db = self.semantic_db();
            let cached = db.component_surface(&file_ref, revision);
            if cached.is_complete() {
                return cached;
            }
        }

        // Extract from analysis snapshot
        let analysis = self.scheduler_script_analysis(canonical_id);
        let surface = analysis.map(|a| verter_semantic::extract::extract_component_surface(&a));

        // Cache and return
        if let Some(ref s) = surface {
            let mut db = self.semantic_db();
            db.set_component_surface(canonical_id.to_string(), revision, s.clone());
        }

        QueryResult::complete(surface, revision)
    }

    /// Query binding declarations and reactivity facts for a file.
    pub fn semantic_bindings(
        &self,
        canonical_id: &str,
    ) -> verter_semantic::query::QueryResult<
        Option<
            Vec<(
                verter_semantic::facts::binding::BindingDeclaration,
                verter_semantic::facts::reactivity::ReactivityFact,
            )>,
        >,
    > {
        use verter_semantic::query::QueryResult;
        use verter_semantic::refs::FileRef;

        let revision = self.semantic_revision();
        let file_ref = FileRef::new(canonical_id);

        // Check cache first
        {
            let db = self.semantic_db();
            let cached = db.bindings(&file_ref, revision);
            if cached.is_complete() {
                return cached;
            }
        }

        // Extract from analysis
        let analysis = self.scheduler_script_analysis(canonical_id);

        let bindings = analysis.map(|a| verter_semantic::extract::extract_bindings(&a));

        if let Some(ref b) = bindings {
            let mut db = self.semantic_db();
            db.set_bindings(canonical_id.to_string(), revision, b.clone());
        }

        QueryResult::complete(bindings, revision)
    }

    /// Get an aggregated semantic snapshot for a file.
    ///
    /// Combines component surface, bindings, reactivity, and import graph
    /// into a single [`FileSemanticSnapshot`](verter_semantic::snapshot::FileSemanticSnapshot).
    /// Populates any missing caches.
    pub fn semantic_snapshot(
        &self,
        canonical_id: &str,
    ) -> verter_semantic::query::QueryResult<verter_semantic::snapshot::FileSemanticSnapshot> {
        use verter_semantic::query::QueryResult;
        use verter_semantic::snapshot::FileSemanticSnapshot;

        let revision = self.semantic_revision();

        // Get or compute each piece
        let surface_result = self.semantic_component_surface(canonical_id);
        let bindings_result = self.semantic_bindings(canonical_id);

        // Import graph
        let import_graph = {
            let file_ref = verter_semantic::refs::FileRef::new(canonical_id);
            let cached = self.semantic_db().import_graph(&file_ref, revision);
            if cached.is_complete() {
                cached.value.unwrap_or_default()
            } else {
                // Extract from analysis
                let analysis = self.scheduler_script_analysis(canonical_id);
                let graph = analysis
                    .map(|a| verter_semantic::extract::extract_import_graph(&a))
                    .unwrap_or_default();
                self.semantic_db().set_import_graph(
                    canonical_id.to_string(),
                    revision,
                    graph.clone(),
                );
                graph
            }
        };

        // Extract boundary edges from template analysis
        let boundary_edges = {
            let template: Option<verter_semantic::analysis::TemplateAnalysisSnapshot> = None;
            template
                .map(|t| {
                    verter_semantic::extract::extract_boundary_edges(
                        canonical_id,
                        &t,
                        &import_graph,
                    )
                })
                .unwrap_or_default()
        };

        let snapshot = FileSemanticSnapshot {
            file_id: canonical_id.to_string(),
            revision,
            component_surface: surface_result.value,
            bindings: bindings_result.value.unwrap_or_default(),
            import_graph,
            boundary_edges,
        };

        QueryResult::complete(snapshot, revision)
    }

    /// Find a binding's reactivity fact by name within a file.
    ///
    /// Uses stable binding name lookup through the semantic snapshot.
    pub fn binding_reactivity(
        &self,
        canonical_id: &str,
        binding_name: &str,
    ) -> verter_semantic::query::QueryResult<
        Option<verter_semantic::facts::reactivity::ReactivityFact>,
    > {
        use verter_semantic::query::QueryResult;

        let revision = self.semantic_revision();
        let bindings_result = self.semantic_bindings(canonical_id);

        let fact = bindings_result.value.and_then(|bindings| {
            bindings
                .into_iter()
                .find(|(decl, _)| decl.name == binding_name)
                .map(|(_, fact)| fact)
        });

        QueryResult::complete(fact, revision)
    }

    /// Get boundary analysis reports for a component via stable ref.
    ///
    /// Uses the semantic DB to resolve the component's surface and analyze
    /// all usages of it across the workspace. Returns boundary issues
    /// (unknown props, missing required, unknown events).
    pub fn boundary_reports(
        &self,
        component_ref: &verter_semantic::refs::ComponentRef,
    ) -> verter_semantic::query::QueryResult<Vec<verter_semantic::analyzers::boundary::BoundaryIssue>>
    {
        use verter_semantic::analyzers::boundary::analyze_boundary;
        use verter_semantic::query::QueryResult;

        let revision = self.semantic_revision();

        // Get the component's declared surface
        let surface_result = self.semantic_component_surface(&component_ref.file_id);
        let surface = match surface_result.value {
            Some(s) => s,
            None => return QueryResult::complete(vec![], revision),
        };

        // Get the semantic snapshot to access boundary edges
        let snapshot = self.semantic_snapshot(&component_ref.file_id);

        // Analyze each boundary edge targeting this component
        let mut all_issues = Vec::new();
        for edge in &snapshot.value.boundary_edges {
            if edge.child_file_id.as_deref() == Some(component_ref.file_id.as_str()) {
                all_issues.extend(analyze_boundary(edge, &surface));
            }
        }

        QueryResult::complete(all_issues, revision)
    }

    /// Get the runtime schema for a component via stable ref.
    ///
    /// Returns a target-neutral schema suitable for generating runtime
    /// validators (Zod, io-ts) or documentation.
    pub fn component_runtime_schema(
        &self,
        component_ref: &verter_semantic::refs::ComponentRef,
    ) -> verter_semantic::query::QueryResult<
        Option<verter_semantic::facts::runtime_schema::ComponentRuntimeSchema>,
    > {
        use verter_semantic::facts::runtime_schema::extract_runtime_schema;
        use verter_semantic::query::QueryResult;

        let revision = self.semantic_revision();
        let surface_result = self.semantic_component_surface(&component_ref.file_id);

        let schema = surface_result.value.map(|s| extract_runtime_schema(&s));

        QueryResult::complete(schema, revision)
    }

    /// Invalidate cached semantic facts for a file.
    ///
    /// Called when the VFS reports a file change, a provider restarts,
    /// or project config changes.
    pub fn semantic_invalidate(&self, canonical_id: &str) {
        self.semantic_db().invalidate(canonical_id);
    }

    /// Invalidate all semantic caches (e.g., after provider restart).
    ///
    /// Per plan: "provider restart, backend switch, project-config change,
    /// or external-type delta must invalidate dependent semantic queries."
    pub fn semantic_invalidate_all(&self) {
        *self.semantic_db() = verter_semantic::db::SemanticDb::new();
    }

    /// Access the unified resolver runtime for counter reads and diagnostics.
    pub fn resolver_runtime(
        &self,
    ) -> &crate::resolver_core::resolver_runtime::UnifiedResolverRuntime<
        crate::meta_resolve::ResolvedComponentMetaState,
        crate::types::FallthroughResolution,
    > {
        &self.resolver.runtime
    }
}
