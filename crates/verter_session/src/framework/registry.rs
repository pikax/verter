#![deny(missing_docs)]
//! The framework adapter registry — the single hub binding each framework's
//! descriptor, carrier leg, synthesis leg, public-API projector, script-fact
//! providers, and surface-resolution disposition.
//!
//! The registry is built ONCE at `VerterHost` construction and is the executor's
//! lookup authority: the wire `framework_adapter_id` a client selects interns to
//! a [`FrameworkAdapterId`], the registry resolves it to a
//! [`FrameworkRegistration`], and the executor drives that registration's legs.
//!
//! Completeness is a closed-set invariant ([`framework_registry_complete`]):
//! every wire [`FrameworkTag`] maps to a registered adapter OR an explicit
//! [`TagDisposition`] row (a deferred vertical or an out-of-scope framework).
//! An unregistered tag is NOT a fabricated registration — the disposition table
//! records the absence explicitly so a new wire tag cannot slip in unhandled.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_compiler::framework_common::FrameworkParseArtifact;
use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
    FrameworkAdapterSemanticVersion, GrammarRegistrationError,
};
use verter_language::{CarrierParse, FileLanguage, FrameworkAdapterId, LanguageId};
use verter_protocol::typeinfo::graph::FrameworkTag;

use crate::framework::api_projector::ComponentApiProjector;
use crate::framework::language_classifier::HostLanguageClassifier;
use crate::framework::surface_store::ErasedFrameworkSurfaceStore;
use crate::framework::synth::ComponentDefaultSynth;
use crate::typeinfo::framework_surface::FrameworkSurfaceAdapter;
use verter_semantic::analysis::framework_facts::{ScriptFactProvider, ScriptFactSyntaxGate};

/// One framework's carrier leg — a monomorphic opener installed at
/// registry-build time.
///
/// Recovers the erased carrier from a registered
/// [`FrameworkParseArtifact`], or `None` for a foreign artifact. No
/// capability token: an opener only opens that adapter's artifacts. A
/// carrier-less adapter has `carrier: None`.
#[derive(Clone, Copy)]
pub struct CarrierLeg {
    /// The registered-projector opener for this adapter.
    pub(crate) open: fn(&FrameworkParseArtifact) -> Option<Arc<dyn CarrierParse>>,
}

impl std::fmt::Debug for CarrierLeg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CarrierLeg").finish_non_exhaustive()
    }
}

/// How an adapter resolves its component surfaces.
///
/// CLOSED two-arm taxonomy: an adapter EITHER ships a plan/normalize
/// [`FrameworkSurfaceAdapter`] ([`Self::Adapter`]) OR registers as
/// [`Self::Deferred`] — a framework whose adapter id is registered but whose
/// surface resolution is not yet implemented (the executor answers every kind
/// structurally UNSUPPORTED for a `Deferred` row). No third arm exists.
pub enum SurfaceRegistration {
    /// A plan/normalize adapter resolves this framework's surfaces.
    Adapter(Arc<dyn FrameworkSurfaceAdapter>),
    /// The adapter id is registered but surface resolution is not yet wired.
    Deferred,
}

impl std::fmt::Debug for SurfaceRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SurfaceRegistration::Adapter(_) => f.write_str("SurfaceRegistration::Adapter"),
            SurfaceRegistration::Deferred => f.write_str("SurfaceRegistration::Deferred"),
        }
    }
}

/// One framework adapter's full registration row.
///
/// Binds the adapter's descriptor to its optional carrier leg, its optional
/// synthesis leg, its optional public-API projector, its script-fact providers,
/// and its surface disposition. Every leg is `Option` (a framework need not
/// supply every capability); `script_fact_providers` is empty for Vue (its macro
/// analysis stays in the shallow pass) and carries one provider for Svelte.
pub struct FrameworkRegistration {
    /// The adapter's static descriptor row.
    pub descriptor: crate::framework::descriptor::FrameworkAdapterDescriptor,
    /// The carrier leg, when the adapter is carrier-backed.
    pub carrier: Option<CarrierLeg>,
    /// The synthesized-default leg, when the adapter synthesizes a `default`.
    pub synth: Option<Arc<dyn ComponentDefaultSynth>>,
    /// The public-API projector leg, when the adapter projects a public-API
    /// virtual file.
    pub api_projector: Option<Arc<dyn ComponentApiProjector>>,
    /// The adapter's syntax-capture script-fact providers (empty for Vue; the
    /// Svelte carrier registers one).
    pub script_fact_providers: Vec<Arc<dyn ScriptFactProvider>>,
    /// How the adapter resolves its component surfaces.
    pub surface: SurfaceRegistration,
    /// The adapter's erased surface-DTO store (one downcast at acquisition by
    /// the owning adapter's executor delegate).
    pub surface_store: Arc<dyn ErasedFrameworkSurfaceStore>,
}

impl std::fmt::Debug for FrameworkRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameworkRegistration")
            .field("descriptor", &self.descriptor)
            .field("carrier", &self.carrier)
            .field("synth", &self.synth.is_some())
            .field("api_projector", &self.api_projector.is_some())
            .field("script_fact_providers", &self.script_fact_providers.len())
            .field("surface", &self.surface)
            .finish_non_exhaustive()
    }
}

/// The disposition of a wire [`FrameworkTag`] the registry walks for
/// completeness.
///
/// A tag is EITHER backed by a registered adapter ([`Self::Registered`]) OR
/// explicitly absent — a [`Self::DeferredVertical`] (its adapter id registers in
/// a later framework vertical) or an [`Self::OutOfScope`] framework (no adapter
/// is planned). The structural non-tags (`NONE` / `OPEN_CANONICAL`) are handled
/// by the completeness guard directly, not through this table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagDisposition {
    /// The tag is backed by a registered adapter id.
    Registered(FrameworkAdapterId),
    /// The tag's adapter is a deferred vertical (registers later).
    DeferredVertical,
    /// The tag's framework is out of scope (no adapter planned).
    OutOfScope,
}

/// The session-side active-provider index — the resolved-validation half's
/// gate-keyed lookup of which script-fact providers are active for a file.
///
/// Rebuilt ONCE per registry construction (a registry rebuild on a
/// capability-snapshot change re-derives it). The two maps mirror the closed
/// [`ScriptFactSyntaxGate`] arms: a file's active set is the union of the
/// providers whose carrier-language gate matches the file's carrier language
/// and the providers whose import-specifier gate matches one of the file's
/// imports.
///
/// EMPTY (when no carrier registers a provider) is a zero-cost fast path:
/// [`Self::is_empty`] short-circuits before any per-file lookup. The Svelte
/// carrier registers one provider (carrier-language gated on `svelte`), so the
/// index is non-empty — but a NON-Svelte file (e.g. a `.vue`) still selects zero
/// providers, keeping its path byte-identical.
#[derive(Default, Clone)]
pub struct ActiveProviderIndex {
    by_carrier_language: FxHashMap<LanguageId, Vec<Arc<dyn ScriptFactProvider>>>,
    by_import_specifier: FxHashMap<&'static str, Vec<Arc<dyn ScriptFactProvider>>>,
}

impl std::fmt::Debug for ActiveProviderIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveProviderIndex")
            .field("by_carrier_language", &self.by_carrier_language.len())
            .field("by_import_specifier", &self.by_import_specifier.len())
            .finish()
    }
}

impl ActiveProviderIndex {
    /// Build the index from every registration's script-fact providers.
    ///
    /// Each provider is filed under its exact-valued [`ScriptFactSyntaxGate`].
    /// With no provider registered the index is empty (the zero-cost path).
    #[must_use]
    pub fn from_registry(registry: &FrameworkAdapterRegistry) -> Self {
        let mut by_carrier_language: FxHashMap<LanguageId, Vec<Arc<dyn ScriptFactProvider>>> =
            FxHashMap::default();
        let mut by_import_specifier: FxHashMap<&'static str, Vec<Arc<dyn ScriptFactProvider>>> =
            FxHashMap::default();
        for registration in registry.registrations.values() {
            for provider in &registration.script_fact_providers {
                match provider.syntax_gate() {
                    ScriptFactSyntaxGate::CarrierLanguage(language) => {
                        by_carrier_language
                            .entry(language)
                            .or_default()
                            .push(Arc::clone(provider));
                    }
                    ScriptFactSyntaxGate::ImportSpecifier(specifier) => {
                        by_import_specifier
                            .entry(specifier)
                            .or_default()
                            .push(Arc::clone(provider));
                    }
                }
            }
        }
        Self {
            by_carrier_language,
            by_import_specifier,
        }
    }

    /// Whether no provider is indexed — the zero-cost fast path. When true the
    /// resolved-validation half does no per-file work at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_carrier_language.is_empty() && self.by_import_specifier.is_empty()
    }

    /// The providers active for a file with carrier language `carrier_language`
    /// and importing `import_specifiers`.
    ///
    /// A provider gated on a carrier language is active when the file's carrier
    /// language matches; a provider gated on an import specifier is active when
    /// the file imports that specifier. A provider already matched by carrier
    /// language is not double-counted by an import-specifier match.
    #[must_use]
    pub fn active_for<'s, I>(
        &self,
        carrier_language: Option<&LanguageId>,
        import_specifiers: I,
    ) -> Vec<Arc<dyn ScriptFactProvider>>
    where
        I: IntoIterator<Item = &'s str>,
    {
        if self.is_empty() {
            return Vec::new();
        }
        let mut active: Vec<Arc<dyn ScriptFactProvider>> = Vec::new();
        let mut seen: Vec<FrameworkAdapterId> = Vec::new();
        if let Some(language) = carrier_language {
            if let Some(providers) = self.by_carrier_language.get(language) {
                for provider in providers {
                    seen.push(provider.adapter_id());
                    active.push(Arc::clone(provider));
                }
            }
        }
        for specifier in import_specifiers {
            if let Some(providers) = self.by_import_specifier.get(specifier) {
                for provider in providers {
                    let id = provider.adapter_id();
                    if seen.contains(&id) {
                        continue;
                    }
                    seen.push(id);
                    active.push(Arc::clone(provider));
                }
            }
        }
        active
    }

    /// Whether a provider's exact-valued [`ScriptFactSyntaxGate`] is active for
    /// a file with carrier language `carrier_language` importing
    /// `import_specifiers`.
    ///
    /// This is the SHARED gate-matching authority: [`Self::active_for`] selects
    /// through the gate-keyed maps it is built from, and the resolved-validation
    /// half's per-registration selection applies this exact predicate over a
    /// registration's own providers — the two agree by construction.
    #[must_use]
    pub fn gate_matches<'s, I>(
        gate: &ScriptFactSyntaxGate,
        carrier_language: Option<&LanguageId>,
        import_specifiers: I,
    ) -> bool
    where
        I: IntoIterator<Item = &'s str>,
    {
        match gate {
            ScriptFactSyntaxGate::CarrierLanguage(language) => carrier_language == Some(language),
            ScriptFactSyntaxGate::ImportSpecifier(specifier) => {
                import_specifiers.into_iter().any(|s| s == *specifier)
            }
        }
    }
}

/// The registered adapter-semantic version of every built-in capability row.
///
/// Version metadata, not framework identity: the catalog composes the SET of
/// carrier grammars, this constant stamps each one. Per-framework version
/// pairs are exactly the branch matrix composition removes — a framework that
/// needs its own version pair is a catalog row, not a host literal.
const BUILT_IN_ADAPTER_SEMANTIC_VERSION: u32 = 1;

/// The registered parser-grammar version of every built-in capability row.
const BUILT_IN_PARSER_GRAMMAR_VERSION: u32 = 1;

/// One composed carrier-grammar capability row: the exact facts the host
/// registers for one adapter × carrier-language pair.
///
/// Identity comes from the compiler's catalog row (`adapter_id` ×
/// `carrier_language_id`), never from a `FileLanguage` literal, so the row
/// is a projection of the capability catalog rather than a restatement of it.
#[derive(Debug, Clone)]
pub struct CarrierGrammarCapability {
    file_language: FileLanguage,
    adapter_semantic_version: FrameworkAdapterSemanticVersion,
    parser_grammar_version: CarrierParserGrammarVersion,
    grammar: CarrierGrammarConfig,
}

impl CarrierGrammarCapability {
    /// The carrier language this row's grammar is registered under.
    #[must_use]
    pub fn file_language(&self) -> &FileLanguage {
        &self.file_language
    }

    /// The adapter semantic version stamped on the registration.
    #[must_use]
    pub fn adapter_semantic_version(&self) -> FrameworkAdapterSemanticVersion {
        self.adapter_semantic_version
    }

    /// The parser grammar version stamped on the registration.
    #[must_use]
    pub fn parser_grammar_version(&self) -> CarrierParserGrammarVersion {
        self.parser_grammar_version
    }

    /// The canonicalized-input carrier grammar config.
    #[must_use]
    pub fn grammar(&self) -> &CarrierGrammarConfig {
        &self.grammar
    }

    /// The adapter this row belongs to.
    #[must_use]
    pub fn adapter_id(&self) -> &FrameworkAdapterId {
        self.file_language
            .adapter_id()
            .expect("a composed capability row is a framework carrier row")
    }
}

/// A frontend capability-catalog row that carries no registered
/// carrier-grammar fact.
///
/// The catalog is the sole authority for which carrier grammars exist, so a
/// row without a grammar is a fail-closed defect, never a skipped framework.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingRegisteredGrammar {
    /// The adapter whose row carried no grammar fact.
    pub adapter_id: FrameworkAdapterId,
    /// The carrier language of that row.
    pub carrier_language_id: LanguageId,
}

impl std::fmt::Display for MissingRegisteredGrammar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "frontend catalog row for adapter '{}' × language '{}' carries no \
             registered grammar fact",
            self.adapter_id, self.carrier_language_id
        )
    }
}

impl std::error::Error for MissingRegisteredGrammar {}

/// The composed framework capability catalog: one [`CarrierGrammarCapability`]
/// per frontend registration the compiler's immutable capability catalog
/// publishes, in that catalog's own deterministic order.
///
/// This is the host's ONE framework enumeration. The compiler catalog states
/// which adapter × carrier-language pairs exist and which grammar each one
/// parses; the host consumes those rows rather than re-listing frameworks, so
/// a framework added upstream registers itself and one removed upstream
/// disappears instead of lingering as a dead row.
#[derive(Debug, Clone)]
pub struct FrameworkCapabilityCatalog {
    rows: Vec<CarrierGrammarCapability>,
}

impl FrameworkCapabilityCatalog {
    /// Compose the catalog from the compiler's built-in frontend capability
    /// registrations.
    ///
    /// # Errors
    ///
    /// [`MissingRegisteredGrammar`] when a catalog row publishes no grammar
    /// fact. Host construction turns that into a construction failure — a host
    /// that silently dropped a row would serve a framework it cannot parse.
    pub fn built_in() -> Result<Self, MissingRegisteredGrammar> {
        let catalog =
            verter_compiler::framework_common::registered_carrier_projection::built_in_frontend_catalog();
        Self::compose(catalog.iter().map(|row| {
            let identity = row.identity();
            (
                identity.adapter_id().clone(),
                identity.carrier_language_id().clone(),
                row.registered_grammar(),
            )
        }))
    }

    /// Compose rows from `(adapter, carrier language, registered grammar)`
    /// triples, in order. The grammar is optional so the missing-fact case is
    /// representable and fails closed instead of being unreachable.
    fn compose<I>(rows: I) -> Result<Self, MissingRegisteredGrammar>
    where
        I: IntoIterator<
            Item = (
                FrameworkAdapterId,
                LanguageId,
                Option<&'static CarrierGrammarConfig>,
            ),
        >,
    {
        let mut composed = Vec::new();
        for (adapter_id, carrier_language_id, grammar) in rows {
            let grammar = grammar.cloned().ok_or_else(|| MissingRegisteredGrammar {
                adapter_id: adapter_id.clone(),
                carrier_language_id: carrier_language_id.clone(),
            })?;
            composed.push(CarrierGrammarCapability {
                file_language: FileLanguage::Framework {
                    adapter_id,
                    language_id: carrier_language_id,
                },
                adapter_semantic_version: FrameworkAdapterSemanticVersion::new(
                    BUILT_IN_ADAPTER_SEMANTIC_VERSION,
                )
                .expect("built-in adapter semantic version is representable"),
                parser_grammar_version: CarrierParserGrammarVersion::new(
                    BUILT_IN_PARSER_GRAMMAR_VERSION,
                )
                .expect("built-in parser grammar version is representable"),
                grammar,
            });
        }
        Ok(Self { rows: composed })
    }

    /// Every composed row, in catalog order.
    #[must_use]
    pub fn rows(&self) -> &[CarrierGrammarCapability] {
        &self.rows
    }

    /// The composed adapter ids, one per row, in catalog order.
    pub fn adapter_ids(&self) -> impl Iterator<Item = &FrameworkAdapterId> {
        self.rows.iter().map(CarrierGrammarCapability::adapter_id)
    }

    /// Whether the catalog composes a row for `adapter_id`.
    #[must_use]
    pub fn contains(&self, adapter_id: &FrameworkAdapterId) -> bool {
        self.rows.iter().any(|row| row.adapter_id() == adapter_id)
    }

    /// Register every composed row's carrier grammar into `authority`.
    ///
    /// The composition is ALL-OR-NOTHING. Every row is first accepted against
    /// a private authority, and only a composition every row survives is
    /// published into `authority`:
    ///
    /// * two rows that resolve to the SAME [`FileLanguage`] are rejected. The
    ///   catalog key is adapter × epoch × capability and does not carry the
    ///   language, so two rows can be distinct catalog entries that share one
    ///   carrier language — registering both would alias them and let the
    ///   later grammar silently replace the earlier one.
    /// * a row the authority rejects is rejected before any row is committed,
    ///   so a failing composition never leaves a half-populated authority.
    ///
    /// # Errors
    ///
    /// [`CarrierGrammarCompositionError`] when a row aliases another row's
    /// carrier language, or when the authority rejects a row. The published
    /// `authority` is untouched by the rejected composition.
    pub fn register_all(
        &self,
        authority: &CarrierGrammarAuthority,
    ) -> Result<(), CarrierGrammarCompositionError> {
        // Private map first: a duplicate carrier language is an aliasing defect
        // the authority cannot report, because its own insert replaces.
        let mut staged: Vec<&CarrierGrammarCapability> = Vec::with_capacity(self.rows.len());
        let mut carrier_languages: FxHashSet<FileLanguage> = FxHashSet::default();
        for row in &self.rows {
            if !carrier_languages.insert(row.file_language.clone()) {
                return Err(CarrierGrammarCompositionError::DuplicateCarrierLanguage(
                    row.file_language.clone(),
                ));
            }
            staged.push(row);
        }
        // Accept every row against a throwaway authority carrying the SAME
        // registration semantics, so a row the live authority would reject is
        // rejected here — with `authority` still empty.
        let probe = CarrierGrammarAuthority::new()
            .map_err(|_| CarrierGrammarCompositionError::AuthorityUnavailable)?;
        for row in &staged {
            probe
                .register_carrier_grammar(
                    row.file_language.clone(),
                    row.adapter_semantic_version,
                    row.parser_grammar_version,
                    row.grammar.clone(),
                )
                .map_err(CarrierGrammarCompositionError::Registration)?;
        }
        for row in staged {
            authority
                .register_carrier_grammar(
                    row.file_language.clone(),
                    row.adapter_semantic_version,
                    row.parser_grammar_version,
                    row.grammar.clone(),
                )
                .map_err(CarrierGrammarCompositionError::Registration)?;
        }
        Ok(())
    }
}

/// Why a composed catalog could not be published into a carrier-grammar
/// authority.
///
/// Every variant is raised BEFORE the composition is published, so an
/// authority that rejected a composition still holds exactly what it held
/// before the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CarrierGrammarCompositionError {
    /// Two composed catalog rows resolve to the same [`FileLanguage`].
    ///
    /// The catalog distinguishes rows by adapter × epoch × capability and does
    /// not carry the carrier language, so distinct rows can share one language
    /// and would alias into a single authority entry — the second grammar
    /// replacing the first, with no error.
    DuplicateCarrierLanguage(FileLanguage),
    /// The authority rejected a row: its grammar does not belong to its
    /// carrier language, its config does not canonicalize, or the authority is
    /// unavailable.
    Registration(GrammarRegistrationError),
    /// The staging authority could not be created, so the composition could
    /// not be accepted before publication.
    AuthorityUnavailable,
}

impl std::fmt::Display for CarrierGrammarCompositionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateCarrierLanguage(language) => write!(
                f,
                "two composed capability-catalog rows resolve to the same carrier language \
                 '{language:?}' — the second registration would alias the first"
            ),
            Self::Registration(error) => {
                write!(f, "carrier-grammar registration rejected: {error:?}")
            }
            Self::AuthorityUnavailable => {
                write!(f, "carrier-grammar staging authority unavailable")
            }
        }
    }
}

impl std::error::Error for CarrierGrammarCompositionError {}

/// The host's explicitly composed framework services.
///
/// One construction step produces the whole framework axis of a host: the
/// capability catalog that names the carrier grammars, and the adapter
/// registry those grammars dispatch through. Both are built here and
/// validated against each other, so a host is never published with a grammar
/// authority and a dispatch authority that disagree about which frameworks
/// exist.
#[derive(Debug)]
pub struct HostServices {
    capabilities: FrameworkCapabilityCatalog,
    registry: FrameworkAdapterRegistry,
}

impl HostServices {
    /// Compose the built-in framework services.
    ///
    /// # Panics
    ///
    /// When a frontend capability row publishes no grammar fact, or when the
    /// two authorities disagree about which frameworks exist. Both mean the
    /// compiler catalog and the adapter legs describe different framework sets;
    /// a partially composed host would serve a framework through one authority
    /// and not the other.
    #[must_use]
    pub fn built_in() -> Self {
        let capabilities = FrameworkCapabilityCatalog::built_in()
            .expect("every built-in frontend capability row publishes a carrier grammar fact");
        let registry = FrameworkAdapterRegistry::built_in();
        // BOTH directions, in production and not only in the unit test: the
        // grammar authority and the dispatch authority must name the same
        // framework set. Catalog-without-registration drops a framework the
        // host claims to parse; registration-without-catalog dispatches a
        // framework the host has no carrier grammar for.
        for adapter_id in capabilities.adapter_ids() {
            assert!(
                registry.contains(adapter_id),
                "the framework adapter registry has no registration for capability-catalog \
                 adapter '{adapter_id}' — the composed host would drop the framework"
            );
        }
        for descriptor in registry.descriptors() {
            assert!(
                capabilities.contains(&descriptor.id),
                "the framework adapter registry registers adapter '{}' that the composed \
                 capability catalog does not name — the composed host would dispatch a \
                 framework it has no carrier grammar for",
                descriptor.id
            );
        }
        Self {
            capabilities,
            registry,
        }
    }

    /// Fail unless `classifier` classifies exactly the framework-carrier
    /// languages this composition registered.
    ///
    /// The classifier is what the host WATCHES and what the language server
    /// ADVERTISES: the watcher globs and the file-operation filters are built
    /// from `classifier.carrier_extensions()`. A classifier that recognises a
    /// carrier this composition registers no grammar for would make the host
    /// watch a framework it cannot serve; a catalog row the classifier never
    /// resolves would make the host serve a framework it never watches. Both
    /// are a disagreement between the host's classification authority and its
    /// composed framework services, and neither is reachable while construction
    /// refuses it.
    ///
    /// # Panics
    ///
    /// When the two disagree, naming the extension or the carrier language
    /// that differs.
    pub fn assert_classifier_agrees(&self, classifier: &HostLanguageClassifier) {
        let carrier_rows = classifier.carrier_rows();
        for (extension, language) in &carrier_rows {
            assert!(
                self.capabilities
                    .rows()
                    .iter()
                    .any(|row| row.file_language() == language),
                "the host classifies '.{extension}' as carrier '{language:?}', which the \
                 composed framework capability catalog does not register — the watcher \
                 would watch a framework the host cannot serve"
            );
        }
        for row in self.capabilities.rows() {
            let language = row.file_language();
            assert!(
                carrier_rows
                    .iter()
                    .any(|(_, classified)| classified == language),
                "the composed framework capability catalog registers carrier \
                 '{language:?}', \
                 which the host's language classifier never resolves — the host would serve \
                 a framework it does not watch"
            );
        }
    }

    /// The composed capability catalog.
    #[must_use]
    pub fn capabilities(&self) -> &FrameworkCapabilityCatalog {
        &self.capabilities
    }

    /// The composed adapter registry.
    #[must_use]
    pub fn framework_registry(&self) -> &FrameworkAdapterRegistry {
        &self.registry
    }
}

/// The framework adapter registry.
///
/// Owns one [`FrameworkRegistration`] per registered adapter id. Built once at
/// host construction; immutable thereafter (a normalizer change is a registry
/// rebuild, not an in-place mutation).
#[derive(Debug)]
pub struct FrameworkAdapterRegistry {
    registrations: FxHashMap<FrameworkAdapterId, FrameworkRegistration>,
    active_provider_index: ActiveProviderIndex,
}

impl FrameworkAdapterRegistry {
    /// Cached framework-surface entries across every adapter's store
    /// (retention observability).
    #[must_use]
    pub fn surface_entry_count(&self) -> usize {
        self.registrations
            .values()
            .map(|registration| registration.surface_store.entry_count())
            .sum()
    }

    /// Build the registry with the production adapter rows.
    #[must_use]
    pub fn built_in() -> Self {
        let mut registrations = FxHashMap::default();
        registrations.insert(FrameworkAdapterId::vue(), vue_registration());
        registrations.insert(FrameworkAdapterId::svelte(), svelte_registration());
        Self::finish(registrations)
    }

    /// Build a registry from explicit registration rows. Used by the in-tree
    /// fixture registration the completeness/deferred/script-fact tests
    /// exercise.
    #[must_use]
    pub fn from_registrations(
        registrations: impl IntoIterator<Item = (FrameworkAdapterId, FrameworkRegistration)>,
    ) -> Self {
        Self::finish(registrations.into_iter().collect())
    }

    /// Seal a registration map into a registry, deriving the active-provider
    /// index once. The index is the ONLY derived state — registrations are
    /// immutable thereafter.
    fn finish(registrations: FxHashMap<FrameworkAdapterId, FrameworkRegistration>) -> Self {
        let mut registry = Self {
            registrations,
            active_provider_index: ActiveProviderIndex::default(),
        };
        registry.active_provider_index = ActiveProviderIndex::from_registry(&registry);
        registry
    }

    /// The active-provider index derived from this registry's script-fact
    /// providers (the resolved-validation half's per-file lookup). The Svelte
    /// carrier contributes one carrier-language-gated provider; Vue contributes
    /// none (its macro analysis stays in the shallow pass).
    #[must_use]
    pub fn active_provider_index(&self) -> &ActiveProviderIndex {
        &self.active_provider_index
    }

    /// The registration for `adapter_id`, if one is registered.
    #[must_use]
    pub fn get(&self, adapter_id: &FrameworkAdapterId) -> Option<&FrameworkRegistration> {
        self.registrations.get(adapter_id)
    }

    /// Whether `adapter_id` is registered.
    #[must_use]
    pub fn contains(&self, adapter_id: &FrameworkAdapterId) -> bool {
        self.registrations.contains_key(adapter_id)
    }

    /// Whether ANY registration carries a syntax-capture script-fact provider —
    /// the registry-wide oracle the [`ActiveProviderIndex`] emptiness mirrors
    /// (the index is empty IFF this is `false`).
    #[must_use]
    pub fn any_provider_registered(&self) -> bool {
        self.registrations
            .values()
            .any(|r| !r.script_fact_providers.is_empty())
    }

    /// Every registered adapter's descriptor, in adapter-id order. The
    /// compiler-completeness guard iterates these to assert every
    /// carrier-bearing descriptor has a registered compile/eval/IDE compiler.
    #[must_use]
    pub fn descriptors(&self) -> Vec<crate::framework::descriptor::FrameworkAdapterDescriptor> {
        let mut rows: Vec<_> = self
            .registrations
            .values()
            .map(|r| r.descriptor.clone())
            .collect();
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        rows
    }

    /// The disposition of a wire framework tag — the completeness oracle.
    ///
    /// A registered tag resolves to [`TagDisposition::Registered`] with its
    /// adapter id; the deferred/out-of-scope tags resolve to their explicit
    /// disposition. The structural non-tags (`NONE` / `OPEN_CANONICAL`) have no
    /// disposition (they are not framework-adapter tags) and return `None`.
    #[must_use]
    pub fn tag_disposition(&self, tag: FrameworkTag) -> Option<TagDisposition> {
        match tag {
            FrameworkTag::Vue => {
                let id = FrameworkAdapterId::vue();
                if self.contains(&id) {
                    Some(TagDisposition::Registered(id))
                } else {
                    // Vue is the keystone adapter; its absence is a build defect,
                    // not a deferred vertical.
                    None
                }
            }
            // Svelte's adapter id is registered with all legs, including the
            // real `SvelteFrameworkAdapter` surface arm; the completeness oracle
            // keys on registration.
            FrameworkTag::Svelte => {
                let id = FrameworkAdapterId::svelte();
                if self.contains(&id) {
                    Some(TagDisposition::Registered(id))
                } else {
                    Some(TagDisposition::DeferredVertical)
                }
            }
            // React / Solid are out of scope (no adapter planned).
            FrameworkTag::React | FrameworkTag::Solid => Some(TagDisposition::OutOfScope),
            // The structural non-tags are not framework-adapter tags.
            FrameworkTag::None | FrameworkTag::OpenCanonical => None,
        }
    }
}

/// The synthesis leg the host's neutral synth-injection selector reaches.
///
/// Selects the registered adapter for `adapter_id` and hands back its
/// [`ComponentDefaultSynth`] leg, or `None` when the adapter has no synth leg.
impl FrameworkAdapterRegistry {
    /// The synthesized-default leg for `adapter_id`, if the adapter registers
    /// one.
    #[must_use]
    pub fn synth_for(
        &self,
        adapter_id: &FrameworkAdapterId,
    ) -> Option<&Arc<dyn ComponentDefaultSynth>> {
        self.get(adapter_id).and_then(|r| r.synth.as_ref())
    }

    /// The public-API projector leg for `adapter_id`, if the adapter registers
    /// one.
    #[must_use]
    pub fn api_projector_for(
        &self,
        adapter_id: &FrameworkAdapterId,
    ) -> Option<&Arc<dyn ComponentApiProjector>> {
        self.get(adapter_id).and_then(|r| r.api_projector.as_ref())
    }

    /// The adapter id of the unique registered adapter that SYNTHESIZES a
    /// `default` component value (carries a [`ComponentDefaultSynth`] leg).
    ///
    /// Exactly one adapter registers a synth leg when Vue is the only carrier;
    /// `None` when none does. Deterministic: when more than one registers, the
    /// lowest adapter id wins so the selection is map-order-independent. The
    /// scratch-injection path uses [`Self::scratch_synthesizing_adapter_id`]
    /// instead — it needs the SPECIFIC carrier-macro inliner, not an arbitrary
    /// `.min()` across every synth-bearing adapter.
    #[must_use]
    pub fn synthesizing_adapter_id(&self) -> Option<FrameworkAdapterId> {
        self.registrations
            .iter()
            .filter(|(_, registration)| registration.synth.is_some())
            .map(|(id, _)| id.clone())
            .min()
    }

    /// The adapter id whose synth leg fabricates the `default` for a typeinfo
    /// EVALUATION SCRATCH (`verter://typeinfo/…`).
    ///
    /// A scratch inlines a `.vue` scope's eval-source as a `.vue`-MACRO prelude
    /// and classifies by its own `.ts` suffix — it has NO resolved framework
    /// language. The macro surface it inlines is Vue's, so the scratch routes to
    /// the VUE synth leg specifically (the carrier-MACRO inliner). This is
    /// REGISTRY DATA (the registered Vue adapter id), not a hardcoded literal,
    /// and not an arbitrary `.min()` over every synth adapter — a `.min()` would
    /// (mis)route the Vue-macro scratch to Svelte once Svelte registers a synth
    /// leg (`"svelte" < "vue"`). `None` when Vue registers no synth leg.
    #[must_use]
    pub fn scratch_synthesizing_adapter_id(&self) -> Option<FrameworkAdapterId> {
        let vue = FrameworkAdapterId::vue();
        self.synth_for(&vue).map(|_| vue)
    }
}

/// The Vue adapter registration row.
fn vue_registration() -> FrameworkRegistration {
    let store: Arc<dyn ErasedFrameworkSurfaceStore> =
        Arc::new(crate::framework::surface_store::FrameworkSurfaceStore::<
            crate::typeinfo::framework_surface::VueSurfaceKey,
            crate::typeinfo::framework_surface::MacroSurfaceDtos,
        >::new());
    FrameworkRegistration {
        descriptor: crate::framework::descriptor::vue_descriptor(),
        carrier: Some(CarrierLeg {
            open: verter_compiler::framework_common::vue_bridge::open_vue_carrier,
        }),
        synth: Some(Arc::new(crate::framework::synth::VueComponentDefaultSynth)),
        api_projector: Some(Arc::new(
            crate::framework::api_projectors::VueComponentApiProjector,
        )),
        script_fact_providers: Vec::new(),
        surface: SurfaceRegistration::Adapter(Arc::new(
            crate::typeinfo::adapters::vue::adapter::VueFrameworkAdapter::default(),
        )),
        surface_store: store,
    }
}

/// The Svelte adapter registration row.
///
/// Registers ALL legs — carrier + synth + script-fact provider + api projector +
/// the real `SvelteFrameworkAdapter` SURFACE leg. The surface store is keyed by
/// the Svelte adapter remainder ([`SvelteSurfaceKey`](crate::typeinfo::framework_surface::SvelteSurfaceKey)
/// — one source family per row).
fn svelte_registration() -> FrameworkRegistration {
    let store: Arc<dyn ErasedFrameworkSurfaceStore> =
        Arc::new(crate::framework::surface_store::FrameworkSurfaceStore::<
            crate::typeinfo::framework_surface::SvelteSurfaceKey,
            crate::typeinfo::framework_surface::MacroSurfaceDtos,
        >::new());
    FrameworkRegistration {
        descriptor: crate::framework::descriptor::svelte_descriptor(),
        carrier: Some(CarrierLeg {
            open: verter_compiler::svelte::carrier::open_svelte_carrier,
        }),
        synth: Some(Arc::new(
            crate::framework::synth::SvelteComponentDefaultSynth,
        )),
        api_projector: Some(Arc::new(
            crate::framework::api_projectors::SvelteComponentApiProjector,
        )),
        script_fact_providers: vec![Arc::new(
            verter_semantic::analysis::framework_facts::svelte::SvelteScriptProvider,
        )],
        surface: SurfaceRegistration::Adapter(Arc::new(
            crate::typeinfo::adapters::svelte::adapter::SvelteFrameworkAdapter::default(),
        )),
        surface_store: store,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_language::carrier_grammar::CarrierAcceptanceError;
    use verter_language::registered_source_authority::{
        CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
    };
    use verter_language::{LanguageRegistry, LanguageRow};

    fn built_in() -> FrameworkAdapterRegistry {
        FrameworkAdapterRegistry::built_in()
    }

    /// COMPLETENESS GUARD: every wire framework tag maps to a registered
    /// adapter OR an explicit deferred/out-of-scope disposition; the structural
    /// non-tags are handled explicitly. A new wire tag with no disposition fails
    /// this (the closed-set invariant).
    #[test]
    fn framework_registry_complete() {
        let registry = built_in();
        // Walk EVERY wire tag — adding a tag forces a disposition decision here.
        for tag in [
            FrameworkTag::None,
            FrameworkTag::Vue,
            FrameworkTag::Svelte,
            FrameworkTag::React,
            FrameworkTag::Solid,
            FrameworkTag::OpenCanonical,
        ] {
            let disposition = registry.tag_disposition(tag);
            match tag {
                // Vue is registered.
                FrameworkTag::Vue => {
                    assert_eq!(
                        disposition,
                        Some(TagDisposition::Registered(FrameworkAdapterId::vue())),
                        "Vue must resolve to a registered adapter"
                    );
                }
                // Svelte's adapter id is REGISTERED with all legs, including the
                // real surface adapter. The completeness oracle keys on
                // registration.
                FrameworkTag::Svelte => {
                    assert_eq!(
                        disposition,
                        Some(TagDisposition::Registered(FrameworkAdapterId::svelte())),
                        "Svelte's adapter id is registered with all legs, including its \
                         surface adapter"
                    );
                }
                // React / Solid are out of scope.
                FrameworkTag::React | FrameworkTag::Solid => {
                    assert_eq!(disposition, Some(TagDisposition::OutOfScope));
                }
                // The structural non-tags have no disposition.
                FrameworkTag::None | FrameworkTag::OpenCanonical => {
                    assert_eq!(
                        disposition, None,
                        "{tag:?} is a structural non-tag, not a framework-adapter tag"
                    );
                }
            }
        }
    }

    /// API-LEG CLAUSE: a descriptor whose import surface is a distinct
    /// suffix-appended API file (it projects a public-API virtual file) MUST
    /// register a public-API projector leg.
    #[test]
    fn descriptor_with_api_suffix_has_a_projector_leg() {
        let registry = built_in();
        for registration in registry.registrations.values() {
            let projects_api = registration
                .descriptor
                .virtual_file_naming
                .as_ref()
                .and_then(|n| n.api_surface_suffix())
                .is_some();
            if projects_api {
                assert!(
                    registration.api_projector.is_some(),
                    "adapter {} projects an API virtual file (api_suffix Some) so it MUST \
                     register an api_projector leg",
                    registration.descriptor.id
                );
            }
        }
    }

    #[test]
    fn vue_registration_carries_every_leg() {
        let registry = built_in();
        let vue = registry
            .get(&FrameworkAdapterId::vue())
            .expect("the Vue adapter is registered");
        assert!(vue.carrier.is_some(), "Vue is carrier-backed");
        assert!(vue.synth.is_some(), "Vue synthesizes a default");
        assert!(vue.api_projector.is_some(), "Vue projects a public API");
        assert!(
            matches!(vue.surface, SurfaceRegistration::Adapter(_)),
            "Vue ships a plan/normalize adapter"
        );
        assert!(
            vue.script_fact_providers.is_empty(),
            "Vue registers no script-fact provider (its macro analysis stays in the shallow pass)"
        );
    }

    #[test]
    fn synth_for_selects_the_vue_leg() {
        let registry = built_in();
        assert!(
            registry.synth_for(&FrameworkAdapterId::vue()).is_some(),
            "the Vue synth leg is reachable by adapter id"
        );
        assert!(
            registry
                .synth_for(&FrameworkAdapterId::new("unregistered"))
                .is_none(),
            "an unregistered adapter id has no synth leg"
        );
    }

    #[test]
    fn scratch_synthesizing_adapter_id_is_vue_the_macro_inliner() {
        // The host's typeinfo-scratch default-injection routes a no-language
        // scratch canonical to the framework whose MACRO surface it inlines —
        // Vue (a scratch inlines a `.vue` macro prelude). That id is
        // REGISTRY-DERIVED (the registered Vue adapter id), NOT a `.min()` over
        // every synth adapter: with Svelte now ALSO registering a synth leg, a
        // `.min()` would (mis)route the Vue-macro scratch to Svelte
        // (`"svelte" < "vue"`).
        let registry = built_in();
        assert_eq!(
            registry.scratch_synthesizing_adapter_id(),
            Some(FrameworkAdapterId::vue()),
            "the typeinfo scratch inlines a Vue macro prelude, so it routes to \
             the Vue synth leg specifically"
        );
        // The generic `.min()` selector now returns Svelte (the lowest synth
        // adapter id) — DISCRIMINATING: it proves the scratch path uses the
        // dedicated selector, not `.min()`, since the two now differ.
        assert_eq!(
            registry.synthesizing_adapter_id(),
            Some(FrameworkAdapterId::svelte()),
            "with two synth adapters the `.min()` selector is Svelte, distinct \
             from the scratch path's Vue routing"
        );
    }

    #[test]
    fn synthesizing_adapter_id_is_none_without_a_synth_leg() {
        // A registry whose adapters carry no synth leg has no synthesizing
        // adapter — the scratch injection no-ops rather than fabricating an id.
        let registry = FrameworkAdapterRegistry::from_registrations([(
            crate::framework::script_facts::fixtures::fixture_adapter_id(),
            crate::framework::script_facts::fixtures::carrier_gated_fixture_registration(),
        )]);
        assert_eq!(
            registry.synthesizing_adapter_id(),
            None,
            "the fixture adapter registers no synth leg, so there is no \
             synthesizing adapter"
        );
    }

    #[test]
    fn built_in_active_provider_index_gates_svelte_only() {
        // The Svelte carrier registers a syntax-capture script-fact provider
        // (carrier-language gated on `svelte`); Vue registers none (its macro
        // analysis stays in the shallow pass). So the index is NON-empty but a
        // Vue file selects ZERO providers — the Vue path stays byte-identical
        // zero-cost.
        let registry = built_in();
        assert!(
            !registry.active_provider_index().is_empty(),
            "the Svelte carrier registers a script-fact provider"
        );
        // A `.vue` file's carrier language selects no provider (Vue is
        // provider-less).
        assert!(
            registry
                .active_provider_index()
                .active_for(Some(&LanguageId::new("vue")), std::iter::empty())
                .is_empty(),
            "a Vue file selects no provider — the Vue path is unchanged"
        );
        // A `.svelte` file's carrier language selects the Svelte provider.
        let active = registry
            .active_provider_index()
            .active_for(Some(&LanguageId::new("svelte")), std::iter::empty());
        assert_eq!(
            active.len(),
            1,
            "a Svelte file selects its one syntax-capture provider"
        );
        assert_eq!(active[0].adapter_id(), FrameworkAdapterId::svelte());
    }

    #[test]
    fn svelte_registration_carries_all_legs_and_a_real_surface_adapter() {
        // The Svelte carrier registers carrier + synth + script-fact provider +
        // api-projector legs PLUS the real `SvelteFrameworkAdapter` SURFACE arm
        // (the executor resolves Svelte surfaces, no longer a Deferred stub).
        let registry = built_in();
        let svelte = registry
            .get(&FrameworkAdapterId::svelte())
            .expect("Svelte is registered");
        assert!(svelte.carrier.is_some(), "Svelte is carrier-backed");
        assert!(svelte.synth.is_some(), "Svelte synthesizes a default");
        assert!(
            svelte.api_projector.is_some(),
            "Svelte projects a public API"
        );
        assert_eq!(
            svelte.script_fact_providers.len(),
            1,
            "Svelte registers its one syntax-capture provider"
        );
        assert!(
            matches!(svelte.surface, SurfaceRegistration::Adapter(_)),
            "Svelte registers a real surface adapter (the Deferred arm is superseded)"
        );
        // The api-leg clause holds: import surface is a distinct `.verter.ts`
        // API file -> api_projector Some.
        assert_eq!(
            svelte
                .descriptor
                .virtual_file_naming
                .as_ref()
                .unwrap()
                .api_surface_suffix(),
            Some(".verter.ts")
        );
    }

    #[test]
    fn active_provider_index_selects_by_carrier_language_gate() {
        let registry = FrameworkAdapterRegistry::from_registrations([(
            crate::framework::script_facts::fixtures::fixture_adapter_id(),
            crate::framework::script_facts::fixtures::carrier_gated_fixture_registration(),
        )]);
        let index = registry.active_provider_index();
        assert!(
            !index.is_empty(),
            "a registered provider populates the index"
        );
        // The fixture provider's carrier-language gate matches its language.
        let active = index.active_for(
            Some(&crate::framework::script_facts::fixtures::fixture_language()),
            std::iter::empty(),
        );
        assert_eq!(active.len(), 1, "the carrier-language gate selects it");
        // A different carrier language does NOT select it.
        let inactive = index.active_for(Some(&LanguageId::new("other")), std::iter::empty());
        assert!(
            inactive.is_empty(),
            "a non-matching carrier language is inert"
        );
    }

    #[test]
    fn active_provider_index_selects_by_import_specifier_gate() {
        let registry = FrameworkAdapterRegistry::from_registrations([(
            crate::framework::script_facts::fixtures::fixture_adapter_id(),
            crate::framework::script_facts::fixtures::import_gated_fixture_registration(),
        )]);
        let index = registry.active_provider_index();
        // Importing the gated specifier selects the provider.
        let active = index.active_for(
            None,
            [crate::framework::script_facts::fixtures::FIXTURE_IMPORT_SPECIFIER],
        );
        assert_eq!(active.len(), 1);
        // Importing something else does not.
        let inactive = index.active_for(None, ["vue"]);
        assert!(inactive.is_empty());
    }

    /// The composed rows ARE the frontend capability rows: every row's
    /// registration identity is the `FileLanguage` the host used to spell by
    /// hand, and the authority accepts each registration on its own identity
    /// validation (a mismatched adapter/language/config triple is rejected).
    #[test]
    fn composed_rows_project_the_frontend_catalog_identities() {
        let catalog = FrameworkCapabilityCatalog::built_in()
            .expect("the built-in frontend catalog publishes a grammar fact per row");
        assert!(
            !catalog.rows().is_empty(),
            "a composed catalog with no rows would leave the host without any carrier grammar"
        );
        for row in catalog.rows() {
            let language = row.file_language();
            assert!(
                language.is_framework_carrier(),
                "a composed capability row must be a framework carrier row, got {language:?}"
            );
            assert!(
                catalog.contains(row.adapter_id()),
                "each row must be reachable by its own adapter id"
            );
        }
        // Composition preserves the rows the host used to enumerate: the
        // registration identity is derived, never re-spelled.
        let vue = catalog
            .rows()
            .iter()
            .find(|row| row.adapter_id() == &FrameworkAdapterId::vue())
            .expect("the frontend catalog publishes the Vue carrier frontend");
        assert_eq!(
            vue.file_language(),
            &FileLanguage::vue(),
            "a composed row must register under the identity its catalog row names"
        );
        let authority = CarrierGrammarAuthority::new().expect("carrier grammar authority");
        catalog
            .register_all(&authority)
            .expect("every composed row passes the authority's own identity validation");
    }

    /// Composition is deterministic: the rows arrive in the frontend
    /// catalog's order, so two hosts compose the same registration sequence.
    #[test]
    fn composed_rows_keep_the_frontend_catalog_order() {
        let first = FrameworkCapabilityCatalog::built_in().expect("composed catalog");
        let second = FrameworkCapabilityCatalog::built_in().expect("composed catalog");
        let ids = |catalog: &FrameworkCapabilityCatalog| -> Vec<String> {
            catalog
                .adapter_ids()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(&first),
            ids(&second),
            "two compositions of the same catalog must agree on row order"
        );
    }

    /// A catalog row that publishes no grammar fact fails closed: the
    /// composition reports the exact adapter × language, never a catalog that
    /// silently omits the framework.
    #[test]
    fn composition_fails_closed_on_a_row_without_a_registered_grammar() {
        let error = FrameworkCapabilityCatalog::compose([(
            FrameworkAdapterId::svelte(),
            LanguageId::new("svelte"),
            None,
        )])
        .expect_err("a row with no registered grammar fact must not compose");
        assert_eq!(error.adapter_id, FrameworkAdapterId::svelte());
        assert_eq!(error.carrier_language_id, LanguageId::new("svelte"));
    }

    /// The two framework authorities of a composed host describe the SAME
    /// framework set, in both directions. A grammar the host parses but
    /// cannot dispatch to — or an adapter it dispatches to but cannot parse —
    /// fails composition instead of half-existing.
    #[test]
    fn host_services_catalog_and_registry_describe_the_same_frameworks() {
        let services = HostServices::built_in();
        let catalog = services.capabilities();
        let registry = services.framework_registry();
        for adapter_id in catalog.adapter_ids() {
            assert!(
                registry.contains(adapter_id),
                "capability-catalog adapter '{adapter_id}' has no adapter registration"
            );
        }
        for descriptor in registry.descriptors() {
            assert!(
                catalog.contains(&descriptor.id),
                "registered adapter '{}' has no composed capability catalog row",
                descriptor.id
            );
        }
    }

    /// Two catalog rows that resolve to ONE carrier language are an identity
    /// alias, not a composition: the authority keys its registrations by
    /// `FileLanguage`, so publishing both would silently let the second
    /// grammar replace the first and still report success. The catalog key is
    /// adapter × epoch × capability and carries no language, so two rows CAN
    /// share one — which is exactly what must be rejected.
    #[test]
    fn register_all_rejects_two_rows_that_share_one_carrier_language() {
        let vue_grammar = registered_vue_grammar();
        let catalog = FrameworkCapabilityCatalog::compose([
            (
                FrameworkAdapterId::vue(),
                LanguageId::new("vue"),
                Some(vue_grammar),
            ),
            (
                FrameworkAdapterId::vue(),
                LanguageId::new("vue"),
                Some(vue_grammar),
            ),
        ])
        .expect("a well-formed row list composes");
        let authority = CarrierGrammarAuthority::new().expect("carrier grammar authority");
        let error = catalog
            .register_all(&authority)
            .expect_err("two rows aliasing one carrier language must not publish");
        assert_eq!(
            error,
            CarrierGrammarCompositionError::DuplicateCarrierLanguage(FileLanguage::vue()),
            "the aliasing row must be named exactly"
        );
    }

    /// A row whose grammar does not belong to its carrier language is rejected
    /// by the authority's own identity rule, and the composition reports it
    /// before publishing. The Vue row ahead of the mismatched Svelte row is
    /// the point: the composition is accepted or rejected as a whole, so the
    /// leading row is never left behind on its own.
    #[test]
    fn register_all_rejects_a_mismatched_row() {
        let catalog = FrameworkCapabilityCatalog::compose([
            (
                FrameworkAdapterId::vue(),
                LanguageId::new("vue"),
                Some(registered_vue_grammar()),
            ),
            // Svelte carrier language carrying the Vue grammar.
            (
                FrameworkAdapterId::svelte(),
                LanguageId::new("svelte"),
                Some(registered_vue_grammar()),
            ),
        ])
        .expect("a well-formed row list composes");
        let authority = CarrierGrammarAuthority::new().expect("carrier grammar authority");
        let error = catalog
            .register_all(&authority)
            .expect_err("a grammar that does not belong to its carrier language must not publish");
        assert_eq!(
            error,
            CarrierGrammarCompositionError::Registration(
                GrammarRegistrationError::ConfigLanguageMismatch
            )
        );
        // The leading Vue row must not have been committed: the authority has
        // no Vue registration, so no Vue source is accepted against it.
        let source_authority = RegisteredSourceAuthority::new().expect("source authority");
        let vue_source = source_authority
            .register_source(
                CanonicalFileId::new("file:///workspace/App.vue"),
                FileIncarnation::new(1),
                SourceGeneration::new(1),
                FileLanguage::vue(),
                Arc::from("<template><p/></template>"),
            )
            .expect("registered source");
        assert_eq!(
            authority
                .accept_registered_source(&source_authority, &vue_source, registered_vue_grammar())
                .err(),
            Some(CarrierAcceptanceError::NoRegisteredGrammar),
            "a rejected composition must leave the caller authority untouched"
        );
        // The same authority still accepts the composition the host publishes.
        FrameworkCapabilityCatalog::built_in()
            .expect("built-in catalog")
            .register_all(&authority)
            .expect("the built-in composition publishes into a fresh authority");
    }

    /// The host's watch surface IS its classifier's carrier rows, so the
    /// classifier and the composed catalog must name the same carriers. The
    /// built-in host agrees; a classifier that recognises a carrier the
    /// catalog never registered fails construction instead of producing a
    /// watcher for a framework the host cannot serve.
    #[test]
    fn host_classifier_and_composed_catalog_agree_on_carriers() {
        let services = HostServices::built_in();
        services.assert_classifier_agrees(&HostLanguageClassifier::with_built_in_registry(
            crate::framework::ProjectCapabilitySnapshot::empty(),
        ));
    }

    #[test]
    #[should_panic(expected = "watch a framework the host cannot serve")]
    fn a_classifier_row_the_catalog_never_registered_fails_the_host() {
        let services = HostServices::built_in();
        let mut rows = builtin_registry_rows();
        rows.push(LanguageRow::fixed(
            "rax",
            FileLanguage::Framework {
                adapter_id: FrameworkAdapterId::new("rax"),
                language_id: LanguageId::new("rax"),
            },
        ));
        services.assert_classifier_agrees(&classifier_over(rows));
    }

    #[test]
    #[should_panic(expected = "a framework it does not watch")]
    fn a_catalog_carrier_the_classifier_never_resolves_fails_the_host() {
        let services = HostServices::built_in();
        let mut rows = builtin_registry_rows();
        rows.retain(|row| row.extension != "svelte");
        services.assert_classifier_agrees(&classifier_over(rows));
    }

    /// The compiler catalog's own Vue grammar, so a test row spells the same
    /// grammar the host registers rather than a hand-built stand-in.
    fn registered_vue_grammar() -> &'static CarrierGrammarConfig {
        verter_compiler::framework_common::registered_carrier_projection::registered_grammar_for(
            &FrameworkAdapterId::vue(),
            &LanguageId::new("vue"),
        )
        .expect("the frontend catalog publishes the Vue carrier grammar")
    }

    fn builtin_registry_rows() -> Vec<LanguageRow> {
        vec![
            LanguageRow::fixed("vue", FileLanguage::vue()),
            LanguageRow::fixed("svelte", FileLanguage::svelte()),
        ]
    }

    fn classifier_over(rows: Vec<LanguageRow>) -> HostLanguageClassifier {
        HostLanguageClassifier::new(
            std::sync::Arc::new(LanguageRegistry::new(rows)),
            crate::framework::ProjectCapabilitySnapshot::empty(),
        )
    }
}
