//! The source-worker boundary. Retained ASTs stay on their worker; every
//! answer crosses the request boundary as owned IR or a typed refusal.

use super::request_bound::{RequestBoundAdapter, RequestBoundLifecycle};
use std::sync::Arc;
use verter_session_query::flow::binding::FlowBindingMapError;
use verter_session_query::flow::bundle::{
    FlowSliceFunctionKey, FlowSourceIdentity, KeyedFunctionStructure,
};
use verter_session_query::type_solver::{PreparedTypeDecl, PreparedValueDecl};
use verter_session_query::{QueryHostAdmission, QueryHostError, QueryHostServe};
use verter_type_engine::resolver_core::request_ports::OwnedLowering;
use verter_type_expr::locators::AuthoredBodyLocator;

pub(crate) fn observe_whole_content(
    canonical: &str,
    hash: verter_session_query::analysis::types::Hash16,
) {
    verter_type_engine::resolver_core::resolver_context::observe_fan_out(
        verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
            canonical_id: canonical.into(),
            hash,
        },
    );
}

fn observe_source_content(source: &super::ShallowFileState) {
    observe_whole_content(&source.source_identity.canonical, source.whole_hash);
}

pub(crate) fn observe_analysis_revision(
    canonical: &str,
    revision: &verter_session_query::analysis::file_analysis::AnalysisSourceRevision,
) {
    match revision.whole_hash() {
        Some(hash) => observe_whole_content(canonical, hash),
        None => verter_type_engine::fact_tracing::note_non_cacheable_read_fan_out(
            verter_session_query::facts::reuse::NonCacheableReadReason::UnobservableSource,
        ),
    }
}

fn lower_authored(
    inputs: &(impl SourceInputProvider + KeptBodyHolder),
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
    observe_whole_content(canonical, serve.indexed.whole_hash);
    let memo = serve.indexed.shallow_state.decl_bodies();
    QueryHostServe {
        admission: QueryHostAdmission::from_store_published(serve.store_published),
        outcome: deref_with_kept(inputs, memo, locator)
            .map(crate::query_host_port::neutral_lowering)
            .map_err(crate::query_host_port::neutral_error),
    }
}

/// Acquire the prepared flow structure `key` names, bound to that key.
///
/// The key is a request: every one of its axes is admitted against the
/// serving artifact's live source identity — canonical, function, both
/// body hashes, parse environment, exact parse identity, language row and
/// this process's toolchain — before the structure is built, and the
/// product is bound to the key through the same admission. A key differing
/// on any axis is a typed miss, never a product published under it.
fn prepare_structure(
    inputs: &impl SourceInputProvider,
    key: &FlowSliceFunctionKey,
) -> Result<Option<KeyedFunctionStructure>, FlowBindingMapError> {
    let Some(serve) = inputs.raw_serve(&key.canonical_id) else {
        return Ok(None);
    };
    let indexed = serve.indexed;
    observe_whole_content(&key.canonical_id, indexed.whole_hash);
    let memo = indexed.shallow_state.decl_bodies();
    let index = crate::host_source_demand::consume_walked_read(memo.function_program_index());
    let Some(matched) = index.get(&key.function) else {
        return Ok(None);
    };
    let entry = matched.entry();
    let Some(parse_key) = indexed.source_parse_key() else {
        return Ok(None);
    };
    let source = FlowSourceIdentity {
        canonical_id: &key.canonical_id,
        parse_env_hash: indexed.parse_env_hash,
        parse_key: &parse_key,
        file_language: &indexed.file_language,
        build_toolchain_fingerprint:
            verter_session_query::source::toolchain::current_build_toolchain_fingerprint(),
    };
    if source.admits(key, entry).is_err() {
        return Ok(None);
    }
    let Some(prepared) =
        crate::host_source_demand::consume_walked_read(memo.function_flow_structure(entry))?
    else {
        return Ok(None);
    };
    Ok(KeyedFunctionStructure::bind(key.clone(), prepared, entry, source).ok())
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
        Option<
            verter_retention::heap_size::OwnedValue<
                verter_session_query::type_solver::PreparedTypeDecl,
            >,
        >,
        verter_session_query::inputs::prepared::PreparationFailure,
    >;
    fn raw_prepared_value_decl(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<
            verter_retention::heap_size::OwnedValue<
                verter_session_query::type_solver::PreparedValueDecl,
            >,
        >,
        verter_session_query::inputs::prepared::PreparationFailure,
    >;
    fn source_host(&self) -> &crate::VerterHost;
    fn source_session_view(&self) -> Option<&dyn crate::session_view::SessionView>;
    /// The shared type-route policy's publication for this provider's
    /// resolution view: admitted, unretained, or refused.
    fn type_dependency_publication(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> verter_workspace::ResolutionPublication<String>;

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
        input: &verter_session_query::inputs::prepared::PreparedInputRecord,
    ) -> Option<crate::resolver_core::CachedValue<super::prepared_decl::PreparedDeclBundle>>;
    fn indexed(
        &self,
        input: &verter_session_query::inputs::indexed::IndexedInputRecord,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>>;
}
impl<L: RequestBoundLifecycle> SourceInputProvider for RequestBoundAdapter<L>
where
    Self: verter_type_engine::resolver_core::resolver_context::sealed::Sealed
        + verter_type_engine::resolver_core::resolver_context::sealed::RequestBoundSealed,
{
    #[inline]
    fn raw_prepared_type_decl(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Result<
        Option<verter_retention::heap_size::OwnedValue<PreparedTypeDecl>>,
        verter_session_query::inputs::prepared::PreparationFailure,
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
        Option<verter_retention::heap_size::OwnedValue<PreparedValueDecl>>,
        verter_session_query::inputs::prepared::PreparationFailure,
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
    fn type_dependency_publication(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> verter_workspace::ResolutionPublication<String> {
        self.0
            .type_dependency_publication(owner_canonical, import_source)
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
        input: &verter_session_query::inputs::prepared::PreparedInputRecord,
    ) -> Option<crate::resolver_core::CachedValue<super::prepared_decl::PreparedDeclBundle>> {
        self.0
            .request_view()
            .overlay()
            .input_artifacts
            .prepared(input)
    }
    fn indexed(
        &self,
        input: &verter_session_query::inputs::indexed::IndexedInputRecord,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        self.0
            .request_view()
            .overlay()
            .input_artifacts
            .indexed(input)
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
        Option<verter_retention::heap_size::OwnedValue<PreparedTypeDecl>>,
        verter_session_query::inputs::prepared::PreparationFailure,
    > {
        let seed = crate::VerterHost::resolver_store_view(self).into_cold_seed_view();
        crate::VerterHost::prepared_type_decl_in_with_store_view::<verter_audit::observe::Uncaptured>(
            self,
            &verter_type_engine::request_observers::CaptureTargets::EMPTY,
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
        Option<verter_retention::heap_size::OwnedValue<PreparedValueDecl>>,
        verter_session_query::inputs::prepared::PreparationFailure,
    > {
        let seed = crate::VerterHost::resolver_store_view(self).into_cold_seed_view();
        crate::VerterHost::prepared_value_decl_in_with_store_view::<verter_audit::observe::Uncaptured>(
            self,
            &verter_type_engine::request_observers::CaptureTargets::EMPTY,
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
    fn type_dependency_publication(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> verter_workspace::ResolutionPublication<String> {
        crate::VerterHost::resolve_type_dependency_canonical(self, owner_canonical, import_source)
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
        input: &verter_session_query::inputs::prepared::PreparedInputRecord,
    ) -> Option<crate::resolver_core::CachedValue<super::prepared_decl::PreparedDeclBundle>> {
        self.source_input_leases.prepared(input)
    }
    fn indexed(
        &self,
        input: &verter_session_query::inputs::indexed::IndexedInputRecord,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        self.source_input_leases.indexed(input)
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
    verter_type_engine::fact_tracing::note_non_cacheable_read_fan_out(
        verter_session_query::facts::reuse::NonCacheableReadReason::LeaseMiss,
    );
}
/// Implements the engine's expression-source selection for one concrete
/// source-input provider (each provider is named; the port is foreign to
/// this crate).
macro_rules! expression_source_selection_from_source_inputs {
    ([$($generics:tt)*] $ty:ty $(where [$($bounds:tt)*])?) => {
        impl<$($generics)*> verter_type_engine::resolver_core::request_ports::ExpressionSourceSelection for $ty $(where $($bounds)*)? {
            type ExpressionDemand = crate::host_source_demand::HostExpressionDemand;

            fn indexed_flow_source(
                &self,
                canonical: &str,
            ) -> Option<(
                verter_session_query::inputs::indexed::IndexedInputServe,
                Option<Self::ExpressionDemand>,
            )> {
                verter_type_engine::count_resolver_context_call!("ExpressionSourceSelection::indexed_flow_source");
                let serve =
                    verter_type_engine::resolver_core::request_ports::IndexedInputs::ensure_indexed_ready_serve(self, canonical)?;
                let demand = SourceInputProvider::source(self, &serve.indexed.shallow_state).map(|source| {
                    crate::host_source_demand::HostExpressionDemand::new(
                        source.decl_bodies().indexed_expression_demand(),
                    )
                });
                Some((serve, demand))
            }

            fn indexed_expression_source(
                &self,
                canonical: &str,
            ) -> Option<(
                verter_session_query::inputs::indexed::IndexedInputServe,
                Self::ExpressionDemand,
            )> {
                verter_type_engine::count_resolver_context_call!("ExpressionSourceSelection::indexed_expression_source");
                let serve =
                    verter_type_engine::resolver_core::request_ports::IndexedInputs::ensure_indexed_ready_serve(self, canonical)?;
                let source = SourceInputProvider::source(self, &serve.indexed.shallow_state)?;
                Some((
                    serve,
                    crate::host_source_demand::HostExpressionDemand::new(
                        source.decl_bodies().indexed_expression_demand(),
                    ),
                ))
            }
        }
    };
}
expression_source_selection_from_source_inputs!([L: RequestBoundLifecycle] RequestBoundAdapter<L> where [
    Self: verter_type_engine::resolver_core::resolver_context::sealed::Sealed
        + verter_type_engine::resolver_core::resolver_context::sealed::RequestBoundSealed,
]);
#[cfg(any(test, feature = "test-support"))]
expression_source_selection_from_source_inputs!([] crate::VerterHost);

/// Session-side extension over a request context: what host framework code
/// reads beside the engine's request ports. The semantic engine never demands
/// these, so they are not part of its ports.
pub(crate) trait HostSourcePort {
    /// The Svelte resolved-validation script facts of `canonical`.
    fn svelte_script_facts(
        &self,
        canonical: &str,
    ) -> crate::framework::script_facts::ScriptFactEvidence<
        verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts,
    >;
    /// The framework parse artifact behind a served indexed input, from the
    /// request lease that retains it. Engine inputs carry only the owned
    /// parse facts; host framework code that needs the parsed carrier itself
    /// reads it here.
    fn framework_parse_artifact(
        &self,
        input: &verter_session_query::inputs::indexed::IndexedInputRecord,
    ) -> Option<Arc<verter_compiler::framework_common::FrameworkParseArtifact>>;
    /// The retained source's function program index — a test probe over the
    /// source lease the request retains.
    #[cfg(test)]
    fn function_program_index(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
    ) -> Option<Arc<verter_session_query::function_program::FunctionProgramIndex>>;
    /// The owner's `TypeDecl` with its body already lowered — the eager
    /// type-body demand the `typeinfo::oracle_core` source walk reads. Gated to
    /// that consumer (`#[cfg(any(test, feature = "oracle-gen"))]`, see
    /// `typeinfo/mod.rs`), so a shipped build carries no unread method.
    #[cfg(any(test, feature = "oracle-gen"))]
    fn lowered_type_decl(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<verter_semantic_source::decl_body_memo::LoweredTypeDecl>>;
    /// The owner's raw-source surfaces in one `SymbolSpace` — the escape-free
    /// read of the syntactic symbol inventory the `typeinfo::oracle_core`
    /// source walk consumes. Same gate as [`Self::lowered_type_decl`].
    #[cfg(any(test, feature = "oracle-gen"))]
    fn raw_source_surfaces(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
        space: verter_parser::utils::oxc::script::raw_surface::SymbolSpace,
    ) -> Option<verter_semantic_source::decl_body_memo::RawSurfaces>;
}
impl<T: SourceInputProvider> HostSourcePort for T {
    fn svelte_script_facts(
        &self,
        canonical: &str,
    ) -> crate::framework::script_facts::ScriptFactEvidence<
        verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts,
    > {
        SourceInputProvider::raw_svelte_script_facts(self, canonical)
    }
    fn framework_parse_artifact(
        &self,
        input: &verter_session_query::inputs::indexed::IndexedInputRecord,
    ) -> Option<Arc<verter_compiler::framework_common::FrameworkParseArtifact>> {
        SourceInputProvider::indexed(self, input)?
            .framework_parse
            .clone()
    }
    #[cfg(test)]
    fn function_program_index(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
    ) -> Option<Arc<verter_session_query::function_program::FunctionProgramIndex>> {
        let Some(source) = SourceInputProvider::source(self, source) else {
            missing_source();
            return None;
        };
        Some(crate::host_source_demand::consume_walked_read(
            source.decl_bodies().function_program_index(),
        ))
    }
    #[cfg(any(test, feature = "oracle-gen"))]
    fn lowered_type_decl(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<verter_semantic_source::decl_body_memo::LoweredTypeDecl>> {
        let Some(source) = SourceInputProvider::source(self, source) else {
            missing_source();
            return None;
        };
        source.type_decl_in(owner, name)
    }
    #[cfg(any(test, feature = "oracle-gen"))]
    fn raw_source_surfaces(
        &self,
        source: &verter_session_query::inputs::shallow::ShallowInputRecord,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
        space: verter_parser::utils::oxc::script::raw_surface::SymbolSpace,
    ) -> Option<verter_semantic_source::decl_body_memo::RawSurfaces> {
        let Some(source) = SourceInputProvider::source(self, source) else {
            missing_source();
            return None;
        };
        Some(source.decl_bodies().raw_surfaces_for_in(owner, name, space))
    }
}

/// A session request context: the engine's request contract plus the
/// session-only extensions a framework-surface resolver demands.
///
/// `M` is the context family's capture execution.
pub(crate) trait HostRequestContext<
    M: verter_audit::observe::ObserveExec = verter_audit::observe::Uncaptured,
>:
    verter_type_engine::resolver_core::ResolverContext<super::HostCapabilities<M>> + HostSourcePort
{
}
impl<
        M: verter_audit::observe::ObserveExec,
        T: verter_type_engine::resolver_core::ResolverContext<super::HostCapabilities<M>>
            + HostSourcePort,
    > HostRequestContext<M> for T
{
}

/// Implements the engine's owned-lowering port for one concrete source-input
/// provider (each provider is named; the port is foreign to this crate).
macro_rules! owned_lowering_from_source_inputs {
    ([$($generics:tt)*] $ty:ty $(where [$($bounds:tt)*])?) => {
        impl<$($generics)*> OwnedLowering for $ty $(where $($bounds)*)? {
            fn class_member_input(
                &self,
                canonical: &str,
                owner: verter_type_expr::TopLevelOwnerId,
                class: &str,
                name: &str,
            ) -> Option<verter_session_query::source::deref::ClassMemberInput> {
                use verter_session_query::facts::{FactKey, FactLane, SymbolSpace};
                use verter_type_engine::resolver_core::request_ports::RouteLookup;
                use verter_type_expr::locators::*;
                if owner != verter_type_expr::TopLevelOwnerId::ordinary_file() { return None; }
                let serve = SourceInputProvider::raw_serve(self, canonical)?;
                if !serve.store_published {
                    verter_type_engine::fact_tracing::note_non_cacheable_read_fan_out(verter_session_query::facts::reuse::NonCacheableReadReason::FencedServe);
                }
                let source = &serve.indexed.shallow_state;
                if verter_session_query::inputs::contributors::classify_shallow_module_kind(source)
                    == verter_session_query::inputs::contributors::FileModuleKind::Script {
                    let answer = self.global_contributor_answer(canonical, class, SymbolSpace::Type);
                    verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                        verter_session_query::facts::fact_cache::FactVersionRef::RouteSurface(
                            verter_session_query::facts::fact_cache::RouteSurfaceFactRef {
                                canonical_id: canonical.into(),
                                key: verter_session_query::inputs::contributors::population_contributor_fact_key(
                                    &verter_session_query::resolution::AugmentationTargetKind::GlobalAugmentation, class),
                                lane: FactLane::Semantic,
                                expected_hash: answer.population_fingerprint,
                            }));
                    if answer.contributors.entries.len() != 1 || !answer.contributors.entries.iter().all(|entry|
                        entry.artifact_key.canonical.as_ref() == canonical && entry.owner == owner) { return None; }
                }
                let fact = self.parse_fact_for_observed_content(canonical, serve.indexed.whole_hash,
                    FactKey::Member { exporter: class.into(), name: verter_type_expr::facts::FactPropertyKey::identifier(name), space: SymbolSpace::Type }, FactLane::Semantic)?;
                if fact.expected_hash == [0;16] { return None; }
                let locator = AuthoredBodyLocator::DeclBody(TypeBodySlot {
                    anchor: AuthoredAnchor { canonical_id: canonical.into(), owner, symbol: class.into(), space: LocatorSymbolSpace::Type },
                    path: vec![TypeBodyPathStep::ClassMembersNamed { name_hash: class_member_name_hash(name) }].into(),
                });
                let selected = source.decl_bodies().deref_locator_body(&locator).ok()?;
                let verter_session_query::source::deref::DerefedBodyShape::Single(body) = &selected.shape else { return None; };
                let refs = verter_type_expr::referenced_names(body);
                let mut name_resolution = rustc_hash::FxHashMap::default();
                for reference in refs.type_names {
                    if reference.qualified || selected.type_parameters.iter().any(|param| param.name == reference.head)
                        || source.headers.type_header_in(owner, &reference.head).is_some() { return None; }
                    let target = source.import_target_in(owner, &reference.head)?;
                    if target.is_namespace { return None; }
                    let (publication, witness) = {
                        let scope = crate::host_manage::import_route_witness::ResolutionWitnessScope::enter();
                        let publication = SourceInputProvider::type_dependency_publication(self, canonical, &target.source_specifier);
                        (publication, scope.collected())
                    };
                    let verter_workspace::ResolutionPublication::Admitted(admitted) = publication else { return None; };
                    let dep = admitted.into_result()?;
                    if witness.is_empty() { return None; }
                    let (identity, route_facts) = RouteLookup::resolve_imported_type_root_with_facts(self, &dep, &target.imported_name);
                    let identity = identity?;
                    if identity.canonical_id.as_ref() == canonical || route_facts.is_empty() { return None; }
                    verter_type_engine::resolver_core::resolver_context::observe_fan_out_borrowed(&witness);
                    verter_type_engine::resolver_core::resolver_context::observe_fan_out_borrowed(&route_facts);
                    name_resolution.insert(reference.head.into(), identity);
                }
                let verter_type_expr::TypeExpr::Object(members) = body else { return None; };
                if members.properties.iter().filter(|member| matches!(member, verter_type_expr::ObjectMember::Method(_))).count() > 1 {
                    // Overload groups and accessor pairs retain authored
                    // occurrences and ordered multiplicity on the full path.
                    return None;
                }
                if !members.properties.iter().all(|member|
                    verter_session_query::source::deref::independent_class_member(member, &name_resolution)) { return None; }
                // The Member lens covers authored references; the route fact
                // covers their owner-qualified binding geometry.
                let route = self.parse_fact_for_observed_content(canonical, serve.indexed.whole_hash, FactKey::SyntacticRouteInterface, FactLane::Semantic)?;
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(verter_session_query::facts::fact_cache::FactVersionRef::Parse(route));
                let display = self.parse_fact_for_observed_content(canonical, serve.indexed.whole_hash,
                    fact.key.clone(), FactLane::Display)?;
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(verter_session_query::facts::fact_cache::FactVersionRef::Parse(fact));
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(verter_session_query::facts::fact_cache::FactVersionRef::Parse(display));
                Some(verter_session_query::source::deref::ClassMemberInput { selected, name_resolution })
            }
            fn script_setup_type_params(
                &self,
                serve: &verter_session_query::inputs::indexed::IndexedInputServe,
            ) -> Vec<verter_type_expr::TypeParam> {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::script_setup_type_params");
                crate::host_resolve::sfc_script_setup_type_params(
                    &serve.indexed.raw_source,
                    HostSourcePort::framework_parse_artifact(self, &serve.indexed).as_deref(),
                )
            }
            fn terminal_macro_inventory(
                &self,
                canonical: &str,
            ) -> verter_type_engine::resolver_core::request_ports::TerminalMacroInventory {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::terminal_macro_inventory");
                let indexed = verter_type_engine::resolver_core::request_ports::IndexedInputs::indexed_for_current_content(self, canonical);
                let base_source = (indexed.is_none() && SourceInputProvider::source_session_view(self).is_none())
                    .then(|| SourceInputProvider::source_host(self).scheduler_source(canonical))
                    .flatten();
                verter_type_engine::resolver_core::request_ports::TerminalMacroInventory {
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
                Option<verter_retention::heap_size::OwnedValue<verter_session_query::type_solver::PreparedValueDecl>>,
                verter_session_query::inputs::prepared::PreparationFailure,
            > {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::prepared_value_decl");
                SourceInputProvider::raw_prepared_value_decl(self, canonical_id, owner, symbol_name)
            }

            fn prepared_type_decl(
                &self,
                canonical_id: &str,
                owner: verter_type_expr::TopLevelOwnerId,
                symbol_name: &str,
            ) -> Result<
                Option<verter_retention::heap_size::OwnedValue<verter_session_query::type_solver::PreparedTypeDecl>>,
                verter_session_query::inputs::prepared::PreparationFailure,
            > {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::prepared_type_decl");
                SourceInputProvider::raw_prepared_type_decl(self, canonical_id, owner, symbol_name)
            }

            fn augmentation_index_with_resolutions(
                &self,
                target: crate::file_artifact_store::AugmentationTargetKind,
            ) -> (
                crate::file_artifact_store::AugmentationTargetKey,
                Arc<verter_session_query::resolution::AugmenterSet>,
                verter_type_engine::resolver_core::request_ports::AugmenterResolutions,
            ) {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::augmentation_index");
                let host = SourceInputProvider::source_host(self);
                host.ingest_program_ambient_roots();
                let env = verter_type_engine::resolver_core::request_ports::IndexedInputs::host_view_env_hashes(self);
                let (population, discriminator) =
                    crate::session_view::augmentation_population_for_view(SourceInputProvider::source_session_view(self));
                let key = crate::file_artifact_store::AugmentationTargetKey {
                    project_identity: verter_type_engine::resolver_core::request_ports::IndexedInputs::host_view_project_identity(self),
                    resolve_env_hash: env.resolve_env_hash,
                    lib_env_hash: env.lib_env_hash,
                    population,
                    target,
                };
                let resolutions = std::cell::RefCell::new(
                    verter_type_engine::resolver_core::request_ports::AugmenterResolutions::default(),
                );
                let answer = crate::host_manage::source_augmentation::AugmentationRequestDriver::new(
                    host.project_type_store().indexed(),
                )
                .ensure_populated(
                    &key,
                    |canonical, spec| {
                        verter_type_engine::count_resolver_context_call!(
                            "RouteLookup::resolve_type_dependency_canonical"
                        );
                        let publication =
                            SourceInputProvider::type_dependency_publication(self, canonical, spec);
                        let answer = crate::host_manage::source_augmentation::AugmentationTarget {
                            retainable: matches!(
                                publication,
                                verter_workspace::ResolutionPublication::Admitted(_)
                            ),
                            target: crate::resolver_core::request_bound::type_route_answer(
                                publication,
                            )
                            .map(Arc::from),
                        };
                        resolutions
                            .borrow_mut()
                            .entry(Arc::from(canonical))
                            .or_default()
                            .push((Arc::from(spec), answer.target.clone()));
                        answer
                    },
                    discriminator,
                    Some(&|canonical| {
                        let serve = SourceInputProvider::raw_serve(self, canonical)?;
                        if !serve.store_published {
                            // A fenced serve is a VALID complete read for
                            // this request: the contributor stays in the
                            // population with its exact-content identity,
                            // and the fenced note keeps the enclosing
                            // answer non-cacheable. Only a genuinely
                            // unavailable source (no serve) or superseded
                            // served content (the identity mismatch below)
                            // drops the contributor.
                            verter_type_engine::fact_tracing::note_non_cacheable_read_fan_out(
                                verter_session_query::facts::reuse::NonCacheableReadReason::FencedServe);
                        }
                        let identity = verter_type_engine::resolver_core::request_ports::IndexedInputs::artifact_key_for_current_content(self, canonical)?;
                        (identity.content_hash == serve.indexed.whole_hash).then_some(identity)
                    }),
                );
                (key, answer, resolutions.into_inner())
            }
            fn global_contributor_answer(
                &self,
                demand_canonical: &str,
                name: &str,
                space: verter_session_query::facts::SymbolSpace,
            ) -> verter_type_engine::resolver_core::request_ports::ContributorAnswer {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::global_contributor_answer");
                let host = SourceInputProvider::source_host(self);
                host.ingest_program_ambient_roots();
                let (_, discriminator) = crate::session_view::augmentation_population_for_view(
                    SourceInputProvider::source_session_view(self));
                let index = host.project_type_store().indexed().global_contributor_index();
                let root = host.workspace().published_root();
                let project_for = |canonical: &str| root.as_ref().and_then(|root| host.project_for_canonical_in_snapshot(canonical, &root.snapshot));
                let project = project_for(demand_canonical);
                let lookup = |population: &crate::global_contributors::GlobalContributorPopulation| population.lookup_in_project(name, space, discriminator, project,
                    root.as_ref().map(|root| root.snapshot.generation.0),
                    |entry| {
                        let owner = project_for(entry.artifact_key.canonical.as_ref());
                        project.and_then(|project| root.as_ref().and_then(|root| host.program_lists(&root.snapshot, project, entry.artifact_key.canonical.as_ref())))
                            .unwrap_or(owner.is_none() || owner == project)
                    },
                    |entries| host.declaration_order(demand_canonical, entries.iter().map(|entry| entry.artifact_key.canonical.as_ref())));
                let mut population = index.snapshot();
                let (mut contributors, mut order) = lookup(&population);
                // Walking the program indexes the files it reaches; once
                // they joined the population, read it again, so the first
                // read answers what every later one does.
                if order.is_some() {
                    let grown = index.snapshot();
                    if !Arc::ptr_eq(&grown, &population) {
                        population = grown;
                        (contributors, order) = lookup(&population);
                    }
                }
                if let Some(order) = order {
                    verter_type_engine::resolver_core::resolver_context::observe_program_file_order(&order);
                }
                verter_type_engine::resolver_core::request_ports::ContributorAnswer {
                    population_fingerprint: population.observation_fingerprint(
                        &crate::file_artifact_store::AugmentationTargetKind::GlobalAugmentation,
                        name, discriminator),
                    contributors,
                }
            }
            fn contributor_answer(
                &self,
                anchor: &str,
                target: &crate::file_artifact_store::AugmentationTargetKind,
                name: &str,
                space: Option<verter_session_query::facts::SymbolSpace>,
            ) -> verter_type_engine::resolver_core::request_ports::ContributorAnswer {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::contributor_answer");
                let host = SourceInputProvider::source_host(self);
                let (_, discriminator) =
                    crate::session_view::augmentation_population_for_view(SourceInputProvider::source_session_view(self));
                let index = host.project_type_store().indexed().global_contributor_index();
                let read = |population: &crate::global_contributors::GlobalContributorPopulation| match space {
                    Some(space) => population.lookup_in_space(target, name, discriminator, true, space),
                    None => population.lookup(target, name, discriminator, true),
                };
                // The libraries the program loads are indexed — so in the
                // population — before it is read; an order that walks the
                // program indexes the files it reaches, and the population
                // is read again once they joined it.
                host.settle_program_libraries(anchor);
                let mut population = index.snapshot();
                let mut contributors = read(&population);
                let ordered = host.declaration_order(
                    anchor,
                    contributors.entries.iter().map(|entry| entry.artifact_key.canonical.as_ref()),
                );
                if ordered.is_some() {
                    let grown = index.snapshot();
                    if !Arc::ptr_eq(&grown, &population) {
                        population = grown;
                        contributors = read(&population);
                    }
                }
                let population_fingerprint =
                    population.observation_fingerprint(target, name, discriminator);
                verter_type_engine::resolver_core::request_ports::ContributorAnswer {
                    population_fingerprint,
                    contributors,
                }
            }
            fn augmentation_contributor_order(
                &self,
                anchor: &str,
                key: &crate::file_artifact_store::AugmentationTargetKey,
                augmenters: &verter_session_query::resolution::AugmenterSet,
                name: &str,
                contributors: &verter_session_query::inputs::contributors::SymbolContributors,
                selection: verter_session_query::inputs::contributors::FileScopeContribution,
            ) -> Arc<[verter_session_query::inputs::contributors::AugmentationCandidate]> {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::augmentation_contributor_order");
                let host = SourceInputProvider::source_host(self);
                let (ordered, order) = host.project_type_store().indexed().global_contributor_index().snapshot()
                    .augmentation_order(key, augmenters, name, contributors, selection,
                        |candidates| {
                            let candidates: Vec<&str> = candidates.collect();
                            host.declaration_order(anchor, candidates.iter().copied())
                        });
                if let Some(order) = order {
                    verter_type_engine::resolver_core::resolver_context::observe_program_file_order(&order);
                }
                ordered
            }
            fn augmenter_artifact_answer(
                &self,
                captured: &verter_session_query::source::artifact_key::FileArtifactKey,
                observed_hash: verter_session_query::analysis::types::Hash16,
            ) -> Option<verter_type_engine::resolver_core::request_ports::AugmenterArtifactAnswer> {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::augmenter_artifact_answer");
                let current = verter_type_engine::resolver_core::request_ports::IndexedInputs::artifact_key_for_current_content(self, &captured.canonical)?;
                if current.content_hash != observed_hash {
                    return None;
                }
                let (artifact, refreshed_key) = SourceInputProvider::source_host(self)
                    .project_type_store()
                    .indexed()
                    .augmenter_artifacts_for_served_identity(captured, &current)?;
                observe_whole_content(&current.canonical, current.content_hash);
                Some(verter_type_engine::resolver_core::request_ports::AugmenterArtifactAnswer {
                    augmentations: Arc::clone(&artifact.augmentations),
                    refreshed_key,
                })
            }
            fn refresh_augmentation_keys(
                &self,
                key: &crate::file_artifact_store::AugmentationTargetKey,
                observed: &verter_session_query::resolution::AugmenterSet,
                refreshed: Vec<(
                    usize,
                    verter_session_query::source::artifact_key::FileArtifactKey,
                )>,
            ) {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::refresh_augmentation_keys");
                SourceInputProvider::source_host(self)
                    .project_type_store()
                    .indexed()
                    .refresh_augmenter_set_keys(key, observed, refreshed);
            }
            #[cfg(any(test, feature = "test-support"))]
            fn augmentation_source_env_forced_unobservable(&self) -> bool {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::augmentation_source_env_forced_unobservable");
                SourceInputProvider::source_host(self)
                    .augmentation_force_source_env_unobservable
                    .load(std::sync::atomic::Ordering::Relaxed)
            }
            fn ordered_sfc_structure(
                &self,
                canonical: &str,
            ) -> Option<verter_session_query::analysis::component_meta::OrderedSfcStructureAnalysis> {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::ordered_sfc_structure");
                let host = SourceInputProvider::source_host(self);
                let structure = match SourceInputProvider::source_session_view(self) {
                    Some(view) => host.registered_structure_for_view(canonical, view),
                    None => host.registered_file_structure(canonical),
                }?;
                let revision = verter_session_query::analysis::file_analysis::AnalysisSourceRevision::of_source(structure.source().bytes());
                observe_analysis_revision(canonical, &revision);
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
                verter_type_engine::count_resolver_context_call!("OwnedLowering::member_presence_for_observed_content");
                let normalized = verter_type_engine::resolver_core::request_ports::IndexedInputs::normalized_analysis_canonical(self, canonical);
                let identity = verter_type_engine::resolver_core::request_ports::IndexedInputs::artifact_key_for_current_content(self, canonical)?;
                if identity.content_hash != observed {
                    return None;
                }
                let artifacts = SourceInputProvider::source_host(self)
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
                verter_type_engine::count_resolver_context_call!("OwnedLowering::parse_fact_for_observed_content");
                let normalized = verter_type_engine::resolver_core::request_ports::IndexedInputs::normalized_analysis_canonical(self, canonical);
                let identity = verter_type_engine::resolver_core::request_ports::IndexedInputs::artifact_key_for_current_content(self, canonical)?;
                if identity.content_hash != observed_hash {
                    return None;
                }
                let expected_hash =
                    SourceInputProvider::observed_fact_hash(self, &normalized, observed_hash, &identity, &key, lane)?;
                Some(verter_session_query::facts::fact_cache::ParseFactRef {
                    canonical_id: canonical.into(),
                    key,
                    lane,
                    expected_hash,
                })
            }

            fn recover_member_spans(
                &self,
                source: &verter_session_query::inputs::shallow::ShallowInputRecord,
                origin: &verter_type_expr::span_origins::MemberSpansOrigin,
            ) -> verter_type_expr::MemberSpans {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::recover_member_spans");
                let Some(source) = SourceInputProvider::source(self, source) else {
                    missing_source();
                    return Default::default();
                };
                observe_source_content(&source);
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
                verter_type_engine::count_resolver_context_call!("OwnedLowering::deref_authored_body");
                let Some(source) = SourceInputProvider::source(self, source) else {
                    missing_source();
                    return Err(verter_session_query::source::deref::LocatorBodyDerefError::LeaseMiss);
                };
                observe_source_content(&source);
                deref_with_kept(self, source.decl_bodies(), locator)
            }

            fn prepared_type_from_input(
                &self,
                input: &verter_session_query::inputs::prepared::PreparedInputRecord,
                owner: verter_type_expr::TopLevelOwnerId,
                name: &str,
            ) -> Result<
                Option<verter_retention::heap_size::OwnedValue<verter_session_query::type_solver::PreparedTypeDecl>>,
                verter_session_query::inputs::prepared::PreparationFailure,
            > {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::prepared_type_from_input");
                let Some(bundle) = SourceInputProvider::prepared(self, input) else {
                    missing_source();
                    return Ok(None);
                };
                bundle.prepared_type_decls.get_in(owner, name)
            }
            fn batch_type_bodies(
                &self,
                demands: &[verter_session_query::source::demand::TypeBodyBatchDemand<'_>],
            ) -> verter_session_query::source::demand::BatchedBodies {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::batch_type_bodies");
                let sources: Vec<_> = demands
                    .iter()
                    .map(|demand| SourceInputProvider::source(self, demand.source).inspect(|source| observe_source_content(source)))
                    .collect();
                let batch: Vec<_> = demands
                    .iter()
                    .zip(&sources)
                    .filter_map(|(demand, source)| {
                        source.as_ref().map(|source| {
                            verter_semantic_source::decl_body_memo::TypeBodyDemand {
                                memo: source.decl_bodies(),
                                owner: demand.owner,
                                name: demand.name,
                                scope: demand.scope,
                            }
                        })
                    })
                    .collect();
                verter_session_query::source::demand::BatchedBodies::new(Box::new(
                    verter_semantic_source::decl_body_memo::prime_type_bodies(&batch),
                ))
            }
            fn prepared_type_for_projection(
                &self,
                input: &verter_session_query::inputs::prepared::PreparedInputRecord,
                owner: verter_type_expr::TopLevelOwnerId,
                name: &str,
            ) -> verter_session_query::inputs::prepared::PreparedTypeDeclResolution {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::prepared_type_for_projection");
                let Some(bundle) = SourceInputProvider::prepared(self, input) else {
                    missing_source();
                    return verter_session_query::inputs::prepared::PreparedTypeDeclResolution::Missing;
                };
                // A declaration first lowered here, for its preparation,
                // keeps its body for the body read the request makes next,
                // so that read does not lower its statements again.
                let holder = KeptBodyHolder::kept_body_holder(self);
                let keep = |body| {
                    if let Some((request, account)) = holder {
                        request.keep_type_body(body, account);
                    }
                };
                bundle.prepared_type_decls.get_in_for_projection_keeping(
                    owner,
                    name,
                    holder.is_some().then_some(&keep as &dyn Fn(_)),
                )
            }
            fn prepare_augmentation_type(
                &self,
                input: &verter_session_query::inputs::prepared::PreparedInputRecord,
                scope: &verter_session_query::declarations::AugmentationScopeKind,
                owner: verter_type_expr::TopLevelOwnerId,
                name: &str,
            ) -> verter_session_query::inputs::prepared::PreparedDeclOutcome<
                verter_session_query::type_solver::PreparedTypeDecl,
            > {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::prepare_augmentation_type");
                let Some(bundle) = SourceInputProvider::prepared(self, input) else {
                    missing_source();
                    return verter_session_query::inputs::prepared::PreparedDeclOutcome::LeaseMiss;
                };
                bundle.prepare_augmentation_type_decl_outcome_in(scope, owner, name)
            }
            fn prepare_augmentation_value(
                &self,
                input: &verter_session_query::inputs::prepared::PreparedInputRecord,
                scope: &verter_session_query::declarations::AugmentationScopeKind,
                owner: verter_type_expr::TopLevelOwnerId,
                name: &str,
            ) -> verter_session_query::inputs::prepared::PreparedDeclOutcome<
                verter_session_query::type_solver::PreparedValueDecl,
            > {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::prepare_augmentation_value");
                let Some(bundle) = SourceInputProvider::prepared(self, input) else {
                    missing_source();
                    return verter_session_query::inputs::prepared::PreparedDeclOutcome::LeaseMiss;
                };
                bundle.prepare_augmentation_value_decl_outcome_in(scope, owner, name)
            }

            fn lower_authored_body(&self, locator: &AuthoredBodyLocator) -> QueryHostServe {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::lower_authored_body");
                lower_authored(self, locator)
            }
            fn prepare_function_structure(
                &self,
                key: &FlowSliceFunctionKey,
            ) -> Result<Option<KeyedFunctionStructure>, FlowBindingMapError> {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::prepare_function_structure");
                prepare_structure(self, key)
            }
            fn transient_type_parts(
                &self,
                source: &verter_session_query::inputs::shallow::ShallowInputRecord,
                owner: verter_type_expr::TopLevelOwnerId,
                name: &str,
            ) -> verter_session_query::source::demand::DemandOutcome<
                verter_session_query::source::transient_parts::TransientTypeParts,
            > {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::transient_type_parts");
                let Some(source) = SourceInputProvider::source(self, source) else {
                    missing_source();
                    return verter_session_query::source::demand::DemandOutcome::LeaseMiss;
                };
                observe_source_content(&source);
                source.decl_bodies().transient_type_parts_in(owner, name)
            }
            fn transient_value_parts(
                &self,
                source: &verter_session_query::inputs::shallow::ShallowInputRecord,
                owner: verter_type_expr::TopLevelOwnerId,
                name: &str,
            ) -> verter_session_query::source::demand::DemandOutcome<
                verter_session_query::source::transient_parts::TransientValueParts,
            > {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::transient_value_parts");
                let Some(source) = SourceInputProvider::source(self, source) else {
                    missing_source();
                    return verter_session_query::source::demand::DemandOutcome::LeaseMiss;
                };
                observe_source_content(&source);
                source.decl_bodies().transient_value_parts_in(owner, name)
            }

            fn lowered_value_decl(
                &self,
                source: &verter_session_query::inputs::shallow::ShallowInputRecord,
                owner: verter_type_expr::TopLevelOwnerId,
                name: &str,
            ) -> Option<Arc<verter_semantic_source::decl_body_memo::LoweredValueDecl>> {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::lowered_value_decl");
                let Some(source) = SourceInputProvider::source(self, source) else {
                    missing_source();
                    return None;
                };
                observe_source_content(&source);
                source.value_decl_in(owner, name)
            }
            fn effective_type_decl(
                &self,
                source: &verter_session_query::inputs::shallow::ShallowInputRecord,
                owner: verter_type_expr::TopLevelOwnerId,
                name: &str,
            ) -> Option<Arc<verter_semantic_source::decl_body_memo::LoweredTypeDecl>> {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::effective_type_decl");
                let Some(source) = SourceInputProvider::source(self, source) else {
                    missing_source();
                    return None;
                };
                observe_source_content(&source);
                source.effective_type_decl_in(owner, name)
            }
            fn effective_value_decl(
                &self,
                source: &verter_session_query::inputs::shallow::ShallowInputRecord,
                owner: verter_type_expr::TopLevelOwnerId,
                name: &str,
            ) -> Option<Arc<verter_semantic_source::decl_body_memo::LoweredValueDecl>> {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::effective_value_decl");
                let Some(source) = SourceInputProvider::source(self, source) else {
                    missing_source();
                    return None;
                };
                observe_source_content(&source);
                source.effective_value_decl_in(owner, name)
            }
            fn type_dependencies(
                &self,
                source: &verter_session_query::inputs::shallow::ShallowInputRecord,
                owner: verter_type_expr::TopLevelOwnerId,
                name: &str,
            ) -> Option<Arc<verter_session_query::inputs::shallow::ClassifiedTypeDeps>> {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::type_dependencies");
                let Some(source) = SourceInputProvider::source(self, source) else {
                    missing_source();
                    return None;
                };
                observe_source_content(&source);
                source.type_deps_in(owner, name)
            }
            fn deref_type_argument(
                &self,
                source: &verter_session_query::inputs::shallow::ShallowInputRecord,
                locator: &verter_type_expr::locators::TypeArgLocator,
            ) -> Result<
                verter_type_expr::TypeExpr,
                verter_session_query::source::deref::LocatorBodyDerefError,
            > {
                verter_type_engine::count_resolver_context_call!("OwnedLowering::deref_type_argument");
                let Some(source) = SourceInputProvider::source(self, source) else {
                    missing_source();
                    return Err(verter_session_query::source::deref::LocatorBodyDerefError::LeaseMiss);
                };
                observe_source_content(&source);
                source.decl_bodies().deref_type_arg(locator)
            }
        }
    };
}
/// The request view that holds the type declaration bodies a request's
/// preparations kept for the body reads that follow, and the retention
/// account they are charged to; none outside a request.
trait KeptBodyHolder {
    fn kept_body_holder(
        &self,
    ) -> Option<(
        &crate::resolver_core::RequestStoreView<'_>,
        &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    )>;
}

impl<L: RequestBoundLifecycle> KeptBodyHolder for RequestBoundAdapter<L> {
    fn kept_body_holder(
        &self,
    ) -> Option<(
        &crate::resolver_core::RequestStoreView<'_>,
        &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    )> {
        Some((
            self.0.request_view(),
            self.0.host().project_type_store().retention_account(),
        ))
    }
}

#[cfg(any(test, feature = "test-support"))]
impl KeptBodyHolder for crate::VerterHost {
    fn kept_body_holder(
        &self,
    ) -> Option<(
        &crate::resolver_core::RequestStoreView<'_>,
        &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    )> {
        None
    }
}

/// Deref `locator` through `memo`, reading a type body from the parts
/// `holder`'s request kept for it when it keeps them
/// ([`crate::resolver_core::request_store_view::deref_taking`]).
fn deref_with_kept(
    holder: &impl KeptBodyHolder,
    memo: &verter_semantic_source::decl_body_memo::DeclBodyMemo,
    locator: &AuthoredBodyLocator,
) -> Result<
    verter_session_query::source::deref::DerefedAuthoredBody,
    verter_session_query::source::deref::LocatorBodyDerefError,
> {
    match holder.kept_body_holder() {
        Some((request, _)) => {
            let (derefed, _charges) = crate::resolver_core::request_store_view::deref_taking(
                memo,
                locator,
                |owner, name, scope| request.take_kept_type_body(memo, owner, name, scope),
            );
            derefed
        }
        None => memo.deref_locator_body(locator),
    }
}

owned_lowering_from_source_inputs!([L: RequestBoundLifecycle] RequestBoundAdapter<L> where [
    Self: verter_type_engine::resolver_core::resolver_context::sealed::Sealed
        + verter_type_engine::resolver_core::resolver_context::sealed::RequestBoundSealed,
]);
#[cfg(any(test, feature = "test-support"))]
owned_lowering_from_source_inputs!([] crate::VerterHost);
