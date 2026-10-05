//! Owned input records shared by request ports. Source work stays private to
//! session-owned artifact leases; these records contain only immutable data.
use crate::file_artifact_store::FileArtifactKeySource;
use rustc_hash::FxHashMap;
use std::sync::{Arc, OnceLock};
use verter_session_query::inputs::shallow::ShallowInputRecord;

#[derive(Debug)]
pub(crate) struct CachedProjection<T>(OnceLock<Arc<T>>);
impl<T> Default for CachedProjection<T> {
    fn default() -> Self {
        Self(OnceLock::new())
    }
}
impl<T> Clone for CachedProjection<T> {
    fn clone(&self) -> Self {
        Self::default()
    }
}
impl<T> CachedProjection<T> {
    pub(crate) fn get_or_init(&self, build: impl FnOnce() -> T) -> Arc<T> {
        Arc::clone(self.0.get_or_init(|| Arc::new(build())))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IndexedInputIdentity {
    pub(crate) source: verter_session_query::source::snapshot::SnapshotKey,
    pub(crate) observation_id: u64,
    pub(crate) file_language: verter_language::FileLanguage,
    pub(crate) parse_key: Option<verter_language::ParseKey>,
}

#[derive(Debug, Clone)]
pub struct IndexedInputRecord {
    pub(crate) identity: IndexedInputIdentity,
    pub(crate) whole_hash: verter_session_query::analysis::types::Hash16,
    pub(crate) file_language: verter_language::FileLanguage,
    pub(crate) shallow_state: Arc<ShallowInputRecord>,
    pub(crate) parse_env_hash: verter_session_query::analysis::types::Hash16,
    pub(crate) raw_source: Arc<str>,
    pub(crate) eval_source: Arc<str>,
    pub(crate) framework_parse:
        Option<Arc<verter_compiler::framework_common::FrameworkParseArtifact>>,
    pub(crate) script_analysis:
        Option<Arc<verter_session_query::analysis::script_snapshot::ScriptAnalysisSnapshot>>,
    pub(crate) snapshot: Arc<crate::types::FileAnalysisSnapshot>,
    /// The owner's `interface AppConfig` shallow flag, mirrored from the
    /// artifact onto the request-input record. The authoritative copy is
    /// [`crate::project_type_store::IndexedReady::declares_interface_app_config`];
    /// this mirror exists for the port-shaped consumers, whose only readers
    /// today are the fact-validation proof surfaces (compiled under `test` /
    /// `test-support`). A shipped build therefore carries no reader, so the
    /// mirror is compiled out rather than left as write-only storage.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) declares_interface_app_config: bool,
    route_surface_hash: OnceLock<Option<verter_session_query::analysis::types::Hash16>>,
    source_parse_identity: OnceLock<Option<verter_language::ParseKey>>,
}
impl IndexedInputRecord {
    pub(crate) fn source_parse_key(&self) -> Option<verter_language::ParseKey> {
        self.source_parse_identity
            .get_or_init(|| {
                verter_session_query::source::artifact_key::FileArtifactKey::for_source_identity(
                    Arc::clone(&self.identity.source.canonical),
                    self.whole_hash,
                    &self.raw_source,
                    self.file_language.clone(),
                    self.framework_parse.as_deref(),
                    self.parse_env_hash,
                )
                .map(|key| key.parse_key)
            })
            .clone()
    }
    pub(crate) fn route_surface_hash(
        &self,
    ) -> Option<verter_session_query::analysis::types::Hash16> {
        *self.route_surface_hash.get_or_init(|| {
            self.shallow_state
                .has_resolvable_surface()
                .then(|| crate::resolver_store::hash_route_surface_inputs(&self.shallow_state))
        })
    }
}

impl crate::project_type_store::IndexedReady {
    pub(crate) fn input_record(&self) -> Arc<IndexedInputRecord> {
        self.input_projection.get_or_init(|| IndexedInputRecord {
            identity: IndexedInputIdentity {
                source: self.shallow_state.source_identity.clone(),
                observation_id: {
                    static NEXT_ARTIFACT_OBSERVATION: std::sync::atomic::AtomicU64 =
                        std::sync::atomic::AtomicU64::new(1);
                    NEXT_ARTIFACT_OBSERVATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                },
                file_language: self.file_language.clone(),
                parse_key: self.cached_source_parse_key().flatten(),
            },
            whole_hash: self.whole_hash,
            file_language: self.file_language.clone(),
            shallow_state: self.shallow_state.input_record(),
            parse_env_hash: self.parse_env_hash,
            raw_source: Arc::clone(&self.raw_source),
            eval_source: Arc::clone(&self.eval_source),
            framework_parse: self.framework_parse.clone(),
            script_analysis: self.script_analysis.clone(),
            snapshot: Arc::clone(&self.snapshot),
            #[cfg(any(test, feature = "test-support"))]
            declares_interface_app_config: self.declares_interface_app_config,
            route_surface_hash: OnceLock::new(),
            source_parse_identity: {
                let cell = OnceLock::new();
                if let Some(key) = self.cached_source_parse_key() {
                    let _ = cell.set(key);
                }
                cell
            },
        })
    }
}

#[derive(Clone)]
pub struct IndexedInputServe {
    pub(crate) indexed: Arc<IndexedInputRecord>,
    pub(crate) store_published: bool,
}

#[derive(Clone)]
pub struct PreparedInputRecord {
    pub(crate) observation_id: u64,
    pub(crate) owner_whole_hash: verter_session_query::analysis::types::Hash16,
    pub(crate) owner_scopes:
        Arc<FxHashMap<verter_type_expr::TopLevelOwnerId, super::prepared_decl::PreparedOwnerScope>>,
}
impl PreparedInputRecord {
    pub(crate) fn owner_scope(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
    ) -> Option<&super::prepared_decl::PreparedOwnerScope> {
        self.owner_scopes.get(&owner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_type_store::IndexedReady;
    use crate::resolver_core::ShallowFileState;

    #[test]
    fn request_lowering_pins_fenced_source_after_current_content_changes() {
        use super::super::request_ports::{IndexedInputs, OwnedLowering};
        use crate::types::{HostConfig, UpsertRequest};
        use std::sync::atomic::Ordering::Relaxed;
        let host = crate::VerterHost::new_standalone(HostConfig::default());
        let upsert = |source: &str| {
            host.upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/pin.ts".to_owned(),
                source: Arc::from(source),
                file_language: verter_language::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("source update")
        };
        let _ = upsert("export type Old = { old: string }");
        host.test_force
            .force_indexed_ready_serve_fence_for_tests
            .store(true, Relaxed);
        struct ResetFence<'a>(&'a crate::VerterHost);
        impl Drop for ResetFence<'_> {
            fn drop(&mut self) {
                self.0
                    .test_force
                    .force_indexed_ready_serve_fence_for_tests
                    .store(false, Relaxed);
            }
        }
        let _reset = ResetFence(&host);
        host.with_base_resolver_context(|ctx| {
            let first = IndexedInputs::ensure_indexed_ready_serve(ctx, "/pin.ts").unwrap();
            assert!(!first.store_published);
            let _ = upsert("export type Fresh = { fresh: number }");
            let second = IndexedInputs::ensure_indexed_ready_serve(ctx, "/pin.ts").unwrap();
            assert_ne!(first.indexed.whole_hash, second.indexed.whole_hash);
            let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
            assert!(matches!(
                OwnedLowering::transient_type_parts(
                    ctx,
                    &first.indexed.shallow_state,
                    owner,
                    "Old"
                ),
                verter_session_query::source::demand::DemandOutcome::Ready(Some(_))
            ));
            assert!(matches!(
                OwnedLowering::transient_type_parts(
                    ctx,
                    &first.indexed.shallow_state,
                    owner,
                    "Fresh"
                ),
                verter_session_query::source::demand::DemandOutcome::Ready(None)
            ));
            assert!(matches!(
                OwnedLowering::transient_type_parts(
                    ctx,
                    &second.indexed.shallow_state,
                    owner,
                    "Fresh"
                ),
                verter_session_query::source::demand::DemandOutcome::Ready(Some(_))
            ));
            assert!(
                !first.store_published,
                "source retention cannot promote fenced admission"
            );
        });
    }

    #[test]
    fn cloned_artifacts_retain_distinct_observations() {
        let source: Arc<str> = Arc::from("export type Props = string");
        let first = Arc::new(IndexedReady::new_for_test_with_state(
            [1; 16],
            ShallowFileState::service_backed_for_test_with_hash("/pin.ts", &source, [1; 16]),
            Arc::clone(&source),
            source,
        ));
        let second = Arc::new((*first).clone());
        let leases = InputArtifactLeases::default();
        let first_input = leases.retain(crate::host_manage::prepared_decl::IndexedReadyServe {
            indexed: Arc::clone(&first),
            store_published: false,
        });
        let second_input = leases.retain(crate::host_manage::prepared_decl::IndexedReadyServe {
            indexed: Arc::clone(&second),
            store_published: true,
        });
        assert!(Arc::ptr_eq(
            &first_input.indexed.shallow_state,
            &second_input.indexed.shallow_state
        ));
        assert_ne!(first_input.indexed.identity, second_input.indexed.identity);
        assert!(Arc::ptr_eq(
            &leases.get(&first_input.indexed.identity).unwrap(),
            &first
        ));
        assert!(Arc::ptr_eq(
            &leases.get(&second_input.indexed.identity).unwrap(),
            &second
        ));
        assert!(!first_input.store_published);
    }

    #[test]
    fn retained_input_pins_original_source_after_a_new_version_is_observed() {
        let old_source: Arc<str> = Arc::from("export type Props = { old: string }");
        let new_source: Arc<str> = Arc::from("export type Props = { fresh: number }");
        let old = Arc::new(IndexedReady::new_for_test_with_state(
            [1; 16],
            ShallowFileState::service_backed_for_test_with_hash("/pin.ts", &old_source, [1; 16]),
            Arc::clone(&old_source),
            old_source,
        ));
        let new = Arc::new(IndexedReady::new_for_test_with_state(
            [2; 16],
            ShallowFileState::service_backed_for_test_with_hash("/pin.ts", &new_source, [2; 16]),
            Arc::clone(&new_source),
            new_source,
        ));
        let leases = InputArtifactLeases::default();
        let first = leases.retain(crate::host_manage::prepared_decl::IndexedReadyServe {
            indexed: Arc::clone(&old),
            store_published: false,
        });
        let second = leases.retain(crate::host_manage::prepared_decl::IndexedReadyServe {
            indexed: new,
            store_published: true,
        });
        assert!(!first.store_published);
        assert!(second.store_published);
        let pinned = leases
            .get(&first.indexed.identity)
            .expect("the old source is request-owned");
        assert!(Arc::ptr_eq(&pinned, &old));
        assert!(pinned.shallow_state.has_type_symbol("Props"));
        assert!(!pinned
            .shallow_state
            .decl_bodies()
            .type_entry_materialized("Props"));
        assert!(pinned.shallow_state.type_decl("Props").is_some());
        assert!(
            !first.store_published,
            "lowering cannot promote the original serve"
        );
    }
}

/// Request-owned source lifetimes. Retention does not alter validation-visible
/// completion state or promote the admission carried by an input serve.
#[derive(Default)]
pub(crate) struct InputArtifactLeases {
    retained: Arc<parking_lot::RwLock<RetainedInputs>>,
}
#[derive(Default)]
struct RetainedInputs {
    indexed: FxHashMap<IndexedInputIdentity, Arc<crate::project_type_store::IndexedReady>>,
    sources: FxHashMap<u64, Arc<super::ShallowFileState>>,
    prepared: FxHashMap<u64, Arc<super::prepared_decl::PreparedDeclBundle>>,
}
/// Exact request-selected mirror authority. This fixed operation cannot expose
/// the retained source, its workers, or any session/cache service.
pub(crate) struct MacroMirrorSelector {
    retained: Arc<parking_lot::RwLock<RetainedInputs>>,
    #[cfg(test)]
    forcing: Arc<crate::host_test_force::TestForceKnobs>,
    #[cfg(test)]
    cold_builds: Arc<std::sync::atomic::AtomicUsize>,
}
impl MacroMirrorSelector {
    pub(crate) fn attachment(
        &self,
        identity: &IndexedInputIdentity,
    ) -> Option<crate::structural_carrier_producer::MacroMirrorAttachment> {
        let indexed = self.retained.read().indexed.get(identity).cloned()?;
        Some(indexed.macro_hot_mirror.attach(
            #[cfg(test)]
            Arc::clone(&self.forcing),
            #[cfg(test)]
            Arc::clone(&self.cold_builds),
        ))
    }
}
impl InputArtifactLeases {
    pub(crate) fn macro_selector(
        &self,
        #[cfg(test)] forcing: Arc<crate::host_test_force::TestForceKnobs>,
        #[cfg(test)] cold_builds: Arc<std::sync::atomic::AtomicUsize>,
    ) -> MacroMirrorSelector {
        MacroMirrorSelector {
            retained: Arc::clone(&self.retained),
            #[cfg(test)]
            forcing,
            #[cfg(test)]
            cold_builds,
        }
    }

    pub(crate) fn retain(
        &self,
        serve: crate::host_manage::prepared_decl::IndexedReadyServe,
    ) -> IndexedInputServe {
        let input = serve.indexed.input_record();
        let mut retained = self.retained.write();
        retained
            .sources
            .entry(input.shallow_state.observation_id)
            .or_insert_with(|| Arc::clone(&serve.indexed.shallow_state));
        retained
            .indexed
            .entry(input.identity.clone())
            .or_insert(serve.indexed);
        IndexedInputServe {
            indexed: input,
            store_published: serve.store_published,
        }
    }
    pub(crate) fn retain_shallow(
        &self,
        source: Arc<super::ShallowFileState>,
    ) -> Arc<ShallowInputRecord> {
        let input = source.input_record();
        self.retained
            .write()
            .sources
            .entry(input.observation_id)
            .or_insert(source);
        input
    }

    #[cfg(test)]
    pub(crate) fn get(
        &self,
        identity: &IndexedInputIdentity,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        self.retained.read().indexed.get(identity).cloned()
    }
    pub(crate) fn source(
        &self,
        input: &ShallowInputRecord,
    ) -> Option<Arc<super::ShallowFileState>> {
        let source = self
            .retained
            .read()
            .sources
            .get(&input.observation_id)
            .cloned()?;
        (source.source_identity == input.source_identity).then_some(source)
    }
}

impl std::fmt::Debug for InputArtifactLeases {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let retained = self.retained.read();
        f.debug_struct("InputArtifactLeases")
            .field("indexed", &retained.indexed.len())
            .field("sources", &retained.sources.len())
            .field("prepared", &retained.prepared.len())
            .finish()
    }
}
impl InputArtifactLeases {
    pub(crate) fn retain_prepared(
        &self,
        bundle: Arc<super::prepared_decl::PreparedDeclBundle>,
    ) -> Arc<PreparedInputRecord> {
        let input = bundle.input_record();
        self.retained
            .write()
            .prepared
            .entry(input.observation_id)
            .or_insert(bundle);
        input
    }
    pub(crate) fn prepared(
        &self,
        input: &PreparedInputRecord,
    ) -> Option<Arc<super::prepared_decl::PreparedDeclBundle>> {
        let bundle = self
            .retained
            .read()
            .prepared
            .get(&input.observation_id)
            .cloned()?;
        (bundle.owner_whole_hash == input.owner_whole_hash).then_some(bundle)
    }
}

pub(crate) trait PreparedInputSource {
    fn scope_inputs(&self) -> Arc<PreparedInputRecord>;
}
impl PreparedInputSource for Arc<super::prepared_decl::PreparedDeclBundle> {
    fn scope_inputs(&self) -> Arc<PreparedInputRecord> {
        self.input_record()
    }
}
impl PreparedInputSource for Arc<PreparedInputRecord> {
    fn scope_inputs(&self) -> Arc<PreparedInputRecord> {
        Arc::clone(self)
    }
}

/// Owned outcome of a source declaration demand.
pub enum PreparedDeclOutcome<T> {
    Ready(Option<T>),
    LeaseMiss,
    Failed(super::prepared_decl::PreparationFailure),
}
