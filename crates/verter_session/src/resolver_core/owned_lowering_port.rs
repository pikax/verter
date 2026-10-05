//! The source-worker boundary. Retained ASTs stay on their worker; every
//! answer crosses the request boundary as owned IR or a typed refusal.

use super::request_ports::OwnedLowering;
use super::resolver_context::{RequestBoundAdapter, RequestBoundLifecycle};
use crate::cache_runtime::flow_slice_node::FlowSliceFunctionKey;
use std::sync::Arc;
use verter_session_query::flow::{
    binding::FlowBindingMapError, skeleton::PreparedFunctionBodySkeleton,
};
use verter_session_query::type_solver::{PreparedTypeDecl, PreparedValueDecl};
use verter_session_query::{QueryHostAdmission, QueryHostError, QueryHostServe};
use verter_type_expr::locators::AuthoredBodyLocator;

fn lower_authored(
    inputs: &impl SourceInputProvider,
    locator: &AuthoredBodyLocator,
) -> QueryHostServe {
    let canonical = match locator {
        AuthoredBodyLocator::DeclBody(slot) => &slot.anchor.canonical_id,
        AuthoredBodyLocator::AugmentationBody(body) => &body.anchor.canonical_id,
        AuthoredBodyLocator::JsdocTypedefBody(body) => &body.anchor.canonical_id,
        AuthoredBodyLocator::MacroPayload(body) => &body.anchor.canonical_id,
    };
    let Some(serve) = inputs.raw_serve(canonical) else {
        return QueryHostServe {
            admission: QueryHostAdmission::ReturnOnly,
            outcome: Err(QueryHostError::UnknownFile),
        };
    };
    QueryHostServe {
        admission: QueryHostAdmission::from_store_published(serve.store_published),
        outcome: serve
            .indexed
            .shallow_state
            .decl_bodies()
            .deref_locator_body(locator)
            .map(crate::query_host_port::neutral_lowering)
            .map_err(crate::query_host_port::neutral_error),
    }
}

fn prepare_structure(
    inputs: &impl SourceInputProvider,
    key: &FlowSliceFunctionKey,
) -> Result<Option<PreparedFunctionBodySkeleton>, FlowBindingMapError> {
    let Some(serve) = inputs.raw_serve(&key.canonical_id) else {
        return Ok(None);
    };
    let indexed = serve.indexed;
    let memo = indexed.shallow_state.decl_bodies();
    let index = memo.function_program_index();
    let Some(matched) = index.get(&key.function) else {
        return Ok(None);
    };
    let entry = matched.entry();
    if entry.flow_body_stable_hash != key.flow_body_stable_hash
        || entry.flow_body_exact_hash != Some(key.flow_body_exact_hash)
        || indexed.source_parse_key().as_ref() != Some(&key.parse_key)
        || indexed.file_language != key.file_language
    {
        return Ok(None);
    }
    memo.function_flow_structure(entry)
}

trait SourceInputProvider {
    fn raw_svelte_script_facts(
        &self,
        canonical: &str,
    ) -> crate::framework::script_facts::ScriptFactEvidence<
        verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts,
    >;
    fn raw_prepared_type_decl(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<verter_session_query::type_solver::PreparedTypeDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    >;
    fn raw_prepared_value_decl(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<verter_session_query::type_solver::PreparedValueDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    >;
    fn source_host(&self) -> &crate::VerterHost;
    fn source_session_view(&self) -> Option<&dyn crate::session_view::SessionView>;

    fn observed_fact_hash(
        &self,
        canonical: &str,
        content: verter_session_query::analysis::types::Hash16,
        identity: &verter_session_query::source::artifact_key::FileArtifactKey,
        key: &verter_session_query::facts::registry::FactKey,
        lane: verter_session_query::facts::registry::FactLane,
    ) -> Option<verter_session_query::analysis::types::Hash16>;
    fn raw_serve(
        &self,
        canonical: &str,
    ) -> Option<crate::host_manage::prepared_decl::IndexedReadyServe>;
    fn source(
        &self,
        input: &verter_session_query::inputs::shallow::ShallowInputRecord,
    ) -> Option<Arc<super::ShallowFileState>>;
    fn prepared(
        &self,
        input: &super::request_inputs::PreparedInputRecord,
    ) -> Option<Arc<super::prepared_decl::PreparedDeclBundle>>;
}
impl<L: RequestBoundLifecycle> SourceInputProvider for RequestBoundAdapter<L>
where
    Self: super::resolver_context::sealed::Sealed
        + super::resolver_context::sealed::RequestBoundSealed,
{
    #[inline]
    fn raw_prepared_type_decl(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<PreparedTypeDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    > {
        self.0
            .prepared_type_decl(self, canonical_id, owner, symbol_name)
    }
    #[inline]
    fn raw_prepared_value_decl(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<PreparedValueDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    > {
        self.0
            .prepared_value_decl(self, canonical_id, owner, symbol_name)
    }
    fn raw_svelte_script_facts(
        &self,
        canonical: &str,
    ) -> crate::framework::script_facts::ScriptFactEvidence<
        verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts,
    > {
        self.0
            .host()
            .resolve_svelte_script_facts_with_ctx(self, canonical)
    }
    fn source_host(&self) -> &crate::VerterHost {
        self.0.host()
    }
    fn source_session_view(&self) -> Option<&dyn crate::session_view::SessionView> {
        self.0.session_view()
    }
    fn observed_fact_hash(
        &self,
        canonical: &str,
        content: verter_session_query::analysis::types::Hash16,
        identity: &verter_session_query::source::artifact_key::FileArtifactKey,
        key: &verter_session_query::facts::registry::FactKey,
        lane: verter_session_query::facts::registry::FactLane,
    ) -> Option<verter_session_query::analysis::types::Hash16> {
        self::observed_fact_hash(self.0.host(), canonical, content, identity, key, lane)
    }
    fn raw_serve(
        &self,
        canonical: &str,
    ) -> Option<crate::host_manage::prepared_decl::IndexedReadyServe> {
        let serve = self.0.materialize_indexed_ready_serve(canonical)?;
        self.0.complete_canonical(canonical);
        Some(serve)
    }
    fn source(
        &self,
        input: &verter_session_query::inputs::shallow::ShallowInputRecord,
    ) -> Option<Arc<super::ShallowFileState>> {
        self.0
            .request_view()
            .overlay()
            .input_artifacts
            .source(input)
    }
    fn prepared(
        &self,
        input: &super::request_inputs::PreparedInputRecord,
    ) -> Option<Arc<super::prepared_decl::PreparedDeclBundle>> {
        self.0
            .request_view()
            .overlay()
            .input_artifacts
            .prepared(input)
    }
}
#[cfg(any(test, feature = "test-support"))]
impl SourceInputProvider for crate::VerterHost {
    #[inline]
    fn raw_prepared_type_decl(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<PreparedTypeDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    > {
        let seed = crate::VerterHost::resolver_store_view(self).into_cold_seed_view();
        crate::VerterHost::prepared_type_decl_in_with_store_view(
            self,
            seed.view(),
            None,
            canonical_id,
            owner,
            symbol_name,
        )
    }
    #[inline]
    fn raw_prepared_value_decl(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<PreparedValueDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    > {
        let seed = crate::VerterHost::resolver_store_view(self).into_cold_seed_view();
        crate::VerterHost::prepared_value_decl_in_with_store_view(
            self,
            seed.view(),
            None,
            canonical_id,
            owner,
            symbol_name,
        )
    }
    fn raw_svelte_script_facts(
        &self,
        canonical: &str,
    ) -> crate::framework::script_facts::ScriptFactEvidence<
        verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts,
    > {
        self.resolve_svelte_script_facts_with_ctx(self, canonical)
    }
    fn source_host(&self) -> &crate::VerterHost {
        self
    }
    fn source_session_view(&self) -> Option<&dyn crate::session_view::SessionView> {
        None
    }
    fn observed_fact_hash(
        &self,
        canonical: &str,
        content: verter_session_query::analysis::types::Hash16,
        identity: &verter_session_query::source::artifact_key::FileArtifactKey,
        key: &verter_session_query::facts::registry::FactKey,
        lane: verter_session_query::facts::registry::FactLane,
    ) -> Option<verter_session_query::analysis::types::Hash16> {
        self::observed_fact_hash(self, canonical, content, identity, key, lane)
    }

    fn raw_serve(
        &self,
        canonical: &str,
    ) -> Option<crate::host_manage::prepared_decl::IndexedReadyServe> {
        crate::VerterHost::ensure_indexed_ready_serve(self, canonical)
    }
    fn source(
        &self,
        input: &verter_session_query::inputs::shallow::ShallowInputRecord,
    ) -> Option<Arc<super::ShallowFileState>> {
        self.source_input_leases.source(input)
    }
    fn prepared(
        &self,
        input: &super::request_inputs::PreparedInputRecord,
    ) -> Option<Arc<super::prepared_decl::PreparedDeclBundle>> {
        self.source_input_leases.prepared(input)
    }
}
fn observed_fact_hash(
    host: &crate::VerterHost,
    canonical: &str,
    content: verter_session_query::analysis::types::Hash16,
    identity: &verter_session_query::source::artifact_key::FileArtifactKey,
    key: &verter_session_query::facts::registry::FactKey,
    lane: verter_session_query::facts::registry::FactLane,
) -> Option<verter_session_query::analysis::types::Hash16> {
    let artifacts = host
        .project_type_store()
        .indexed()
        .get_artifacts_for_content(
            canonical,
            content,
            &identity.parse_key,
            &identity.file_language_id,
        )?;
    Some(
        artifacts
            .facts
            .lookup_or_compute(key)
            .map_or([0; 16], |fact| match lane {
                verter_session_query::facts::registry::FactLane::Semantic => fact.semantic_hash,
                verter_session_query::facts::registry::FactLane::Display => fact.display_hash,
            }),
    )
}
fn missing_source() {
    super::resolver_context::note_non_cacheable_read_fan_out(
        super::resolver_context::NonCacheableReadReason::LeaseMiss,
    );
}
impl<
        T: SourceInputProvider
            + super::request_ports::IndexedInputs
            + super::request_ports::RouteLookup,
    > OwnedLowering for T
{
    fn svelte_script_facts(
        &self,
        canonical: &str,
    ) -> crate::framework::script_facts::ScriptFactEvidence<
        verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts,
    > {
        self.raw_svelte_script_facts(canonical)
    }
    fn terminal_macro_inventory(
        &self,
        canonical: &str,
    ) -> super::request_ports::TerminalMacroInventory {
        let indexed = self.indexed_for_current_content(canonical);
        let base_source = (indexed.is_none() && self.source_session_view().is_none())
            .then(|| self.source_host().scheduler_source(canonical))
            .flatten();
        super::request_ports::TerminalMacroInventory {
            origin_whole_hash: indexed
                .as_ref()
                .map(|i| i.whole_hash)
                .or_else(|| base_source.as_ref().map(|s| s.whole_hash)),
            script_analysis: indexed
                .as_ref()
                .and_then(|i| i.script_analysis.clone())
                .or_else(|| {
                    base_source
                        .as_ref()
                        .and_then(|s| s.downcast_data::<crate::host_executor::HostSourceData>())
                        .map(|d| Arc::clone(&d.parse.script_analysis))
                }),
        }
    }
    fn prepared_value_decl(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<verter_session_query::type_solver::PreparedValueDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    > {
        self.raw_prepared_value_decl(canonical_id, owner, symbol_name)
    }

    fn prepared_type_decl(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<Arc<verter_session_query::type_solver::PreparedTypeDecl>>,
        crate::resolver_core::prepared_decl::PreparationFailure,
    > {
        self.raw_prepared_type_decl(canonical_id, owner, symbol_name)
    }

    fn augmentation_index(
        &self,
        target: crate::file_artifact_store::AugmentationTargetKind,
    ) -> (
        crate::file_artifact_store::AugmentationTargetKey,
        Arc<crate::file_artifact_store::AugmenterSet>,
    ) {
        let host = self.source_host();
        host.ingest_program_ambient_roots();
        let env = self.host_view_env_hashes();
        let (population, discriminator) =
            crate::session_view::augmentation_population_for_view(self.source_session_view());
        let key = crate::file_artifact_store::AugmentationTargetKey {
            project_identity: self.host_view_project_identity(),
            resolve_env_hash: env.resolve_env_hash,
            lib_env_hash: env.lib_env_hash,
            population,
            target,
        };
        let answer = crate::host_manage::source_augmentation::AugmentationRequestDriver::new(
            host.project_type_store().indexed(),
        )
        .ensure_populated(
            &key,
            |canonical, spec| {
                self.resolve_type_dependency_canonical(canonical, spec)
                    .map(Arc::from)
            },
            discriminator,
        );
        (key, answer)
    }
    fn global_contributor_answer(
        &self,
        name: &str,
        space: verter_session_query::facts::SymbolSpace,
    ) -> super::request_ports::ContributorAnswer {
        self.source_host().ingest_program_ambient_roots();
        self.contributor_answer(
            &crate::file_artifact_store::AugmentationTargetKind::GlobalAugmentation,
            name,
            true,
            Some(space),
        )
    }
    fn contributor_answer(
        &self,
        target: &crate::file_artifact_store::AugmentationTargetKind,
        name: &str,
        allow_automatic_libs: bool,
        space: Option<verter_session_query::facts::SymbolSpace>,
    ) -> super::request_ports::ContributorAnswer {
        let host = self.source_host();
        let (_, discriminator) =
            crate::session_view::augmentation_population_for_view(self.source_session_view());
        let population = host
            .project_type_store()
            .indexed()
            .global_contributor_index()
            .snapshot();
        let population_fingerprint =
            population.observation_fingerprint(target, name, discriminator);
        let contributors = match space {
            Some(space) => {
                population.lookup_in_space(target, name, discriminator, allow_automatic_libs, space)
            }
            None => population.lookup(target, name, discriminator, allow_automatic_libs),
        };
        super::request_ports::ContributorAnswer {
            population_fingerprint,
            contributors,
        }
    }
    fn augmenter_artifact_answer(
        &self,
        captured: &verter_session_query::source::artifact_key::FileArtifactKey,
        observed_hash: verter_session_query::analysis::types::Hash16,
    ) -> Option<super::request_ports::AugmenterArtifactAnswer> {
        let (artifact, refreshed_key) = self
            .source_host()
            .project_type_store()
            .indexed()
            .augmenter_artifacts_self_healing(captured, observed_hash)?;
        Some(super::request_ports::AugmenterArtifactAnswer {
            augmentations: Arc::clone(&artifact.augmentations),
            refreshed_key,
        })
    }
    fn refresh_augmentation_keys(
        &self,
        key: &crate::file_artifact_store::AugmentationTargetKey,
        observed: &crate::file_artifact_store::AugmenterSet,
        refreshed: Vec<(
            usize,
            verter_session_query::source::artifact_key::FileArtifactKey,
        )>,
    ) {
        if refreshed.is_empty() {
            return;
        }
        let mut entries = observed.entries.clone();
        for (index, artifact_key) in refreshed {
            entries[index].artifact_key = artifact_key;
        }
        self.source_host()
            .project_type_store()
            .indexed()
            .populate_augmenter_set(
                key.clone(),
                Arc::new(crate::file_artifact_store::AugmenterSet {
                    entries,
                    fingerprint: observed.fingerprint,
                }),
            );
    }
    #[cfg(any(test, feature = "test-support"))]
    fn augmentation_source_env_forced_unobservable(&self) -> bool {
        self.source_host()
            .augmentation_force_source_env_unobservable
            .load(std::sync::atomic::Ordering::Relaxed)
    }
    fn ordered_sfc_structure(
        &self,
        canonical: &str,
    ) -> Option<verter_semantic::analysis::component_meta::OrderedSfcStructureAnalysis> {
        let host = self.source_host();
        let structure = match self.source_session_view() {
            Some(view) => host.registered_structure_for_view(canonical, view),
            None => host.registered_file_structure(canonical),
        }?;
        Some(crate::host_resolve::ordered_sfc_structure_analysis(
            &structure,
        ))
    }

    fn member_presence_for_observed_content(
        &self,
        canonical: &str,
        observed: verter_session_query::analysis::types::Hash16,
        key: verter_session_query::facts::registry::FactKey,
    ) -> Option<bool> {
        let normalized = self.normalized_analysis_canonical(canonical);
        let identity = self.artifact_key_for_current_content(canonical)?;
        if identity.content_hash != observed {
            return None;
        }
        let artifacts = self
            .source_host()
            .project_type_store()
            .indexed()
            .get_artifacts_for_content(
                &normalized,
                observed,
                &identity.parse_key,
                &identity.file_language_id,
            )?;
        Some(artifacts.facts.lookup(&key).is_some())
    }
    fn parse_fact_for_observed_content(
        &self,
        canonical: &str,
        observed_hash: verter_session_query::analysis::types::Hash16,
        key: verter_session_query::facts::registry::FactKey,
        lane: verter_session_query::facts::registry::FactLane,
    ) -> Option<verter_session_query::facts::fact_cache::ParseFactRef> {
        let normalized = self.normalized_analysis_canonical(canonical);
        let identity = self.artifact_key_for_current_content(canonical)?;
        if identity.content_hash != observed_hash {
            return None;
        }
        let expected_hash =
            self.observed_fact_hash(&normalized, observed_hash, &identity, &key, lane)?;
        Some(verter_session_query::facts::fact_cache::ParseFactRef {
            canonical_id: canonical.to_owned(),
            key,
            lane,
            expected_hash,
        })
    }

    fn indexed_flow_source(
        &self,
        canonical: &str,
    ) -> Option<(
        super::request_inputs::IndexedInputServe,
        Option<crate::decl_body_memo::IndexedExpressionDemand>,
    )> {
        let serve =
            super::request_ports::IndexedInputs::ensure_indexed_ready_serve(self, canonical)?;
        let demand = self
            .source(&serve.indexed.shallow_state)
            .map(|source| source.decl_bodies().indexed_expression_demand());
        Some((serve, demand))
    }

    fn indexed_expression_source(
        &self,
        canonical: &str,
    ) -> Option<(
        super::request_inputs::IndexedInputServe,
        crate::decl_body_memo::IndexedExpressionDemand,
    )> {
        let serve =
            super::request_ports::IndexedInputs::ensure_indexed_ready_serve(self, canonical)?;
        let source = self.source(&serve.indexed.shallow_state)?;
        let demand = source.decl_bodies().indexed_expression_demand();
        Some((serve, demand))
    }

    fn recover_member_spans(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        origin: &verter_type_expr::span_origins::MemberSpansOrigin,
    ) -> verter_type_expr::MemberSpans {
        let Some(source) = self.source(source) else {
            missing_source();
            return Default::default();
        };
        source.decl_bodies().recover_member_spans_or_absent(origin)
    }

    fn deref_authored_body(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        locator: &AuthoredBodyLocator,
    ) -> Result<
        verter_session_query::source::deref::DerefedAuthoredBody,
        verter_session_query::source::deref::LocatorBodyDerefError,
    > {
        let Some(source) = self.source(source) else {
            missing_source();
            return Err(verter_session_query::source::deref::LocatorBodyDerefError::LeaseMiss);
        };
        source.decl_bodies().deref_locator_body(locator)
    }

    fn prepared_type_from_input(
        &self,
        input: &super::request_inputs::PreparedInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Result<
        Option<Arc<verter_session_query::type_solver::PreparedTypeDecl>>,
        super::prepared_decl::PreparationFailure,
    > {
        let Some(bundle) = self.prepared(input) else {
            missing_source();
            return Ok(None);
        };
        bundle.prepared_type_decls.get_in(owner, name)
    }
    fn prepared_type_for_projection(
        &self,
        input: &super::request_inputs::PreparedInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> super::prepared_decl::PreparedTypeDeclResolution {
        let Some(bundle) = self.prepared(input) else {
            missing_source();
            return super::prepared_decl::PreparedTypeDeclResolution::Missing;
        };
        bundle
            .prepared_type_decls
            .get_in_for_projection(owner, name)
    }
    fn prepare_augmentation_type(
        &self,
        input: &super::request_inputs::PreparedInputRecord,
        scope: &verter_session_query::declarations::AugmentationScopeKind,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> super::prepared_decl::PreparedDeclOutcome<
        verter_session_query::type_solver::PreparedTypeDecl,
    > {
        let Some(bundle) = self.prepared(input) else {
            missing_source();
            return super::prepared_decl::PreparedDeclOutcome::LeaseMiss;
        };
        bundle.prepare_augmentation_type_decl_outcome_in(scope, owner, name)
    }
    fn prepare_augmentation_value(
        &self,
        input: &super::request_inputs::PreparedInputRecord,
        scope: &verter_session_query::declarations::AugmentationScopeKind,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> super::prepared_decl::PreparedDeclOutcome<
        verter_session_query::type_solver::PreparedValueDecl,
    > {
        let Some(bundle) = self.prepared(input) else {
            missing_source();
            return super::prepared_decl::PreparedDeclOutcome::LeaseMiss;
        };
        bundle.prepare_augmentation_value_decl_outcome_in(scope, owner, name)
    }

    fn lower_authored_body(&self, locator: &AuthoredBodyLocator) -> QueryHostServe {
        lower_authored(self, locator)
    }
    fn prepare_function_structure(
        &self,
        key: &FlowSliceFunctionKey,
    ) -> Result<Option<PreparedFunctionBodySkeleton>, FlowBindingMapError> {
        prepare_structure(self, key)
    }
    #[cfg(test)]
    fn function_program_index(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
    ) -> Option<Arc<verter_session_query::function_program::FunctionProgramIndex>> {
        let Some(source) = self.source(source) else {
            missing_source();
            return None;
        };
        Some(source.decl_bodies().function_program_index())
    }

    fn transient_type_parts(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> crate::decl_body_memo::DemandOutcome<crate::decl_body_memo::TransientTypeParts> {
        let Some(source) = self.source(source) else {
            missing_source();
            return crate::decl_body_memo::DemandOutcome::LeaseMiss;
        };
        source.decl_bodies().transient_type_parts_in(owner, name)
    }
    fn transient_value_parts(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> crate::decl_body_memo::DemandOutcome<crate::decl_body_memo::TransientValueParts> {
        let Some(source) = self.source(source) else {
            missing_source();
            return crate::decl_body_memo::DemandOutcome::LeaseMiss;
        };
        source.decl_bodies().transient_value_parts_in(owner, name)
    }

    #[cfg(any(test, feature = "oracle-gen"))]
    fn lowered_type_decl(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<crate::decl_body_memo::LoweredTypeDecl>> {
        let Some(source) = self.source(source) else {
            missing_source();
            return None;
        };
        source.type_decl_in(owner, name)
    }
    fn lowered_value_decl(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<crate::decl_body_memo::LoweredValueDecl>> {
        let Some(source) = self.source(source) else {
            missing_source();
            return None;
        };
        source.value_decl_in(owner, name)
    }
    fn effective_type_decl(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<crate::decl_body_memo::LoweredTypeDecl>> {
        let Some(source) = self.source(source) else {
            missing_source();
            return None;
        };
        source.effective_type_decl_in(owner, name)
    }
    fn effective_value_decl(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<crate::decl_body_memo::LoweredValueDecl>> {
        let Some(source) = self.source(source) else {
            missing_source();
            return None;
        };
        source.effective_value_decl_in(owner, name)
    }
    fn type_dependencies(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<verter_session_query::inputs::shallow::ClassifiedTypeDeps>> {
        let Some(source) = self.source(source) else {
            missing_source();
            return None;
        };
        source.type_deps_in(owner, name)
    }
    #[cfg(any(test, feature = "oracle-gen"))]
    fn raw_source_surfaces(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
        space: verter_parser::utils::oxc::script::raw_surface::SymbolSpace,
    ) -> Option<Arc<Vec<verter_parser::utils::oxc::script::raw_surface::RawSourceSurface>>> {
        let Some(source) = self.source(source) else {
            missing_source();
            return None;
        };
        Some(source.decl_bodies().raw_surfaces_for_in(owner, name, space))
    }
    fn deref_type_argument(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        locator: &verter_type_expr::locators::TypeArgLocator,
    ) -> Result<
        verter_type_expr::TypeExpr,
        verter_session_query::source::deref::LocatorBodyDerefError,
    > {
        let Some(source) = self.source(source) else {
            missing_source();
            return Err(verter_session_query::source::deref::LocatorBodyDerefError::LeaseMiss);
        };
        source.decl_bodies().deref_type_arg(locator)
    }
}
