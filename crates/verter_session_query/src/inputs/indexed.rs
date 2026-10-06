//! Owned indexed-input records: the immutable per-file data the engine reads
//! from one served post-parse artifact. Source work and the artifact itself
//! stay private to the host's request leases; these records carry only owned
//! data.

use std::sync::{Arc, OnceLock};

use verter_language::{FileLanguage, ParseKey};

use crate::analysis::file_analysis::FileAnalysisSnapshot;
use crate::analysis::script_snapshot::ScriptAnalysisSnapshot;
use crate::analysis::types::Hash16;
use crate::inputs::shallow::ShallowInputRecord;
use crate::source::framework_parse::{exact_source_parse_key, FrameworkParseFacts};
use crate::source::snapshot::SnapshotKey;

/// Identity of one observed indexed input: the snapshot it was read from, a
/// per-observation id, and its language / parse identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IndexedInputIdentity {
    pub source: SnapshotKey,
    pub observation_id: u64,
    pub file_language: FileLanguage,
    pub parse_key: Option<ParseKey>,
}

/// The owned record of one served post-parse artifact.
#[derive(Debug, Clone)]
pub struct IndexedInputRecord {
    pub identity: IndexedInputIdentity,
    pub whole_hash: Hash16,
    pub file_language: FileLanguage,
    pub shallow_state: Arc<ShallowInputRecord>,
    pub parse_env_hash: Hash16,
    pub raw_source: Arc<str>,
    pub eval_source: Arc<str>,
    /// The owned facts of the carrier's framework parse, when the file is a
    /// framework carrier.
    pub framework_parse: Option<FrameworkParseFacts>,
    pub script_analysis: Option<Arc<ScriptAnalysisSnapshot>>,
    pub snapshot: Arc<FileAnalysisSnapshot>,
    /// The owner's `interface AppConfig` shallow flag, mirrored from the
    /// artifact onto the request-input record. Its only readers are the
    /// fact-validation proof surfaces compiled for tests, so a shipped build
    /// carries no reader and the mirror is compiled out rather than left as
    /// write-only storage.
    #[cfg(feature = "test-support")]
    pub declares_interface_app_config: bool,
    route_surface_hash: OnceLock<Option<Hash16>>,
    source_parse_identity: OnceLock<Option<ParseKey>>,
}

impl IndexedInputRecord {
    /// Assemble a record. `cached_source_parse_key` is the artifact's already
    /// derived exact parse identity, when it has one (`Some(None)` caches a
    /// refusal); otherwise the identity is derived on first demand.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        identity: IndexedInputIdentity,
        whole_hash: Hash16,
        file_language: FileLanguage,
        shallow_state: Arc<ShallowInputRecord>,
        parse_env_hash: Hash16,
        raw_source: Arc<str>,
        eval_source: Arc<str>,
        framework_parse: Option<FrameworkParseFacts>,
        script_analysis: Option<Arc<ScriptAnalysisSnapshot>>,
        snapshot: Arc<FileAnalysisSnapshot>,
        cached_source_parse_key: Option<Option<ParseKey>>,
    ) -> Self {
        let source_parse_identity = OnceLock::new();
        if let Some(key) = cached_source_parse_key {
            let _ = source_parse_identity.set(key);
        }
        Self {
            identity,
            whole_hash,
            file_language,
            shallow_state,
            parse_env_hash,
            raw_source,
            eval_source,
            framework_parse,
            script_analysis,
            snapshot,
            #[cfg(feature = "test-support")]
            declares_interface_app_config: false,
            route_surface_hash: OnceLock::new(),
            source_parse_identity,
        }
    }

    /// Mirror the artifact's `interface AppConfig` shallow flag.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn with_declares_interface_app_config(mut self, declares: bool) -> Self {
        self.declares_interface_app_config = declares;
        self
    }

    /// The exact parse identity of this record's source under its runtime
    /// language, derived at most once: a carrier reads the key its framework
    /// parse recorded; only a plain script derives one from its source. A
    /// refusal is cached as `None`.
    pub fn source_parse_key(&self) -> Option<ParseKey> {
        self.source_parse_identity
            .get_or_init(|| {
                exact_source_parse_key(
                    &self.raw_source,
                    &self.file_language,
                    self.framework_parse
                        .as_ref()
                        .map(FrameworkParseFacts::carrier_parse_key),
                )
            })
            .clone()
    }

    /// The legacy route-surface digest of this record, computed at most once;
    /// `None` when its shallow state exposes no resolvable surface.
    pub fn route_surface_hash(&self) -> Option<Hash16> {
        *self.route_surface_hash.get_or_init(|| {
            self.shallow_state.has_resolvable_surface().then(|| {
                crate::inputs::route_surface::hash_route_surface_inputs(&self.shallow_state)
            })
        })
    }
}

/// One served indexed input and whether the serving artifact was published
/// to the shared store. A consumer that derives shared-cache entries from the
/// record gates their admission on `store_published`.
#[derive(Clone)]
pub struct IndexedInputServe {
    pub indexed: Arc<IndexedInputRecord>,
    pub store_published: bool,
}
