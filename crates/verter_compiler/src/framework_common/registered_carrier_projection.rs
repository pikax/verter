use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use verter_language::carrier_grammar::{AcceptedRegisteredCarrierSource, CarrierGrammarConfig};
use verter_language::parse_artifact::carrier_inventory::*;
use verter_language::{
    compute_carrier_structure_hash, parse_key_for, CarrierParse, CarrierStructureHash,
    FileLanguage, FrameworkAdapterId, FrameworkParseCommon, LanguageDiagnostic, LanguageId,
    SyntaxReject, UnregisteredFrameworkParseArtifact, UnsupportedSyntaxProfileReason,
    FRAMEWORK_SYNTAX_COMPATIBILITY_DOMAIN, FRAMEWORK_SYNTAX_COMPATIBILITY_EPOCH,
    SVELTE_SYNTAX_COMPATIBILITY_DOMAIN, SVELTE_SYNTAX_COMPATIBILITY_EPOCH,
    VUE_SYNTAX_COMPATIBILITY_DOMAIN, VUE_SYNTAX_COMPATIBILITY_EPOCH,
};
use verter_span::Span;

use verter_language::ParseOptions;

use super::capability::{
    CarrierFrontend, FrameworkEpochId, FrameworkSemanticAuthority, ProjectionBackend,
};
use super::carrier_compiler::{
    CarrierCompiler, CompileUnsupported, RuntimeBlockContentInputs, RuntimeDiagnostic,
    RuntimeDiagnosticSeverity,
};
use super::catalog::{CatalogCapability, CatalogRow, ImmutableCapabilityCatalog};
use super::vue_bridge::VueCarrierCompiler;
use super::vue_carrier_frontend::{
    vue_carrier_frontend_registration, VueCarrierFrontend, VueParseAdmission, VueSfcV3,
};
use super::vue_projection_backend::{
    vue_projection_backend_registration, VueProjectionBackend, VueProjectionError,
    VueProjectionInputs,
};
use super::vue_semantic_authority::{vue_semantic_authority_registration, VueSemanticAuthority};
use crate::compile::types::{VueExecutionInputs, VueMacroSemanticInput};
use crate::compile::RawTemplateData;
use crate::compile_request::CompileRequest;
use crate::standalone::DirectCompileError;
use crate::svelte::carrier_frontend::SvelteParseAdmission;
use crate::svelte::{
    svelte_carrier_frontend_registration, svelte_projection_backend_registration,
    svelte_semantic_authority_registration, SvelteCarrierCompiler, SvelteCarrierFrontend,
    SvelteProjectionBackend, SvelteProjectionError, SvelteProjectionInputs,
    SvelteSemanticAuthority, SvelteSfc5,
};

/// Opaque in-process carrier retained by the registered projector.
///
/// This type's own public API surface has no accessor or downcast method,
/// and it has no serialization, equality, or hashing implementation that
/// could turn it into publication identity. Reaching the erased carrier
/// back out requires calling INTO this module (`FrameworkParseArtifact`'s
/// `pub(crate)` `carrier_ref`/`carrier_arc`/`erased_carrier_for_adapter`
/// methods below) — there is no free-standing token that could be
/// re-minted or handed to an unrelated caller.
#[derive(Clone)]
pub struct RegisteredCarrierPayload {
    inner: Arc<RegisteredCarrierPayloadInner>,
}

/// A framework parse whose geometry was proven by the registered projector.
///
/// The private state and carrier fields make direct construction impossible.
/// Geometry-sensitive consumers receive only this registered form.
pub struct FrameworkParseArtifact {
    adapter_id: FrameworkAdapterId,
    language_id: LanguageId,
    epoch: FrameworkEpochId,
    parse_key: Arc<verter_language::ParseKey>,
    syntax_profile: Arc<verter_language::SyntaxProfileId>,
    common: FrameworkParseCommon,
    carrier_structure_hash: CarrierStructureHash,
    carrier: RegisteredCarrierPayload,
    _geometry: super::registered_geometry_state::RegisteredGeometry,
}

impl FrameworkParseArtifact {
    /// Owning adapter.
    #[must_use]
    pub fn adapter_id(&self) -> &FrameworkAdapterId {
        &self.adapter_id
    }

    /// Concrete language within the adapter.
    #[must_use]
    pub fn language_id(&self) -> &LanguageId {
        &self.language_id
    }

    /// Framework epoch bound on this artifact at registered projection.
    ///
    /// Semantic catalog lookup keys adapter × this epoch × Semantic —
    /// callers do not hop the frontend catalog and do not branch on
    /// Vue/Svelte identity.
    #[must_use]
    pub fn epoch(&self) -> &FrameworkEpochId {
        &self.epoch
    }

    /// Remint catalog epoch identity for miss-path tests. Adapter, carrier
    /// language, geometry, and retained parse stay unchanged.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn remint_epoch_for_tests(&self, epoch: &str) -> Self {
        Self {
            adapter_id: self.adapter_id.clone(),
            language_id: self.language_id.clone(),
            epoch: FrameworkEpochId::new(epoch),
            parse_key: Arc::clone(&self.parse_key),
            syntax_profile: Arc::clone(&self.syntax_profile),
            common: self.common.clone(),
            carrier_structure_hash: self.carrier_structure_hash,
            carrier: self.carrier.clone(),
            _geometry: super::registered_geometry_state::RegisteredGeometry { _private: () },
        }
    }

    /// Exact syntax construction identity.
    #[must_use]
    pub fn parse_key(&self) -> &verter_language::ParseKey {
        &self.parse_key
    }

    /// Normalized parse-option identity.
    #[must_use]
    pub fn syntax_profile(&self) -> &verter_language::SyntaxProfileId {
        &self.syntax_profile
    }

    /// Registered neutral geometry and mapped diagnostics.
    #[must_use]
    pub fn common(&self) -> &FrameworkParseCommon {
        &self.common
    }

    /// Registered carrier inventory.
    #[must_use]
    pub fn inventory(&self) -> &Arc<CarrierBlockInventory> {
        &self.common.inventory
    }

    /// The registered whole-carrier source bytes this artifact's geometry
    /// was proven over: the registered inventory's own witnessed source
    /// space — the same bytes the artifact's parse identity
    /// ([`Self::parse_key`]) was computed from, validated against the
    /// registered source snapshot at inventory construction. Execution
    /// seams that hold an admitted artifact derive their compile source
    /// from here, so the admitted artifact is the single authority for
    /// both geometry and bytes — a caller-supplied byte payload that could
    /// diverge from the admitted parse is unrepresentable.
    #[must_use]
    pub fn carrier_source(&self) -> &Arc<str> {
        self.common
            .inventory
            .source_spaces()
            .iter()
            .find(|space| {
                matches!(
                    space.identity,
                    SourceSpaceIdentity::RegisteredSnapshot { .. }
                )
            })
            .map(SourceSpaceDescriptor::bytes)
            .expect(
                "a registered artifact's inventory holds its witnessed carrier source space \
                 (new_registered consumes exactly one registered source witness)",
            )
    }

    /// Mapped parse diagnostics retained with the registered geometry.
    #[must_use]
    pub fn diagnostics(&self) -> &[LanguageDiagnostic] {
        &self.common.diagnostics
    }

    /// Integrity hash of the registered inventory.
    #[must_use]
    pub fn carrier_structure_hash(&self) -> CarrierStructureHash {
        self.carrier_structure_hash
    }

    /// Adapter-aware compatibility projection derived from registered inventory.
    pub fn script_regions(&self) -> Vec<verter_language::ScriptRegion> {
        self.common
            .script_regions_for_adapter(Some(&self.adapter_id))
    }

    /// Crate-internal reference-form typed carrier recovery: no capability
    /// token — a foreign adapter's artifact carries a DIFFERENT concrete
    /// `CarrierParse` type, so the `Any` downcast already fails
    /// structurally for it. Confined to `verter_compiler` by visibility;
    /// each owning adapter's own inherent methods are the only callers
    /// (each hardcodes its own concrete `T`, so cross-adapter confusion is
    /// not even syntactically possible).
    pub(crate) fn carrier_ref<T: CarrierParse>(&self) -> Option<&T> {
        self.carrier
            .inner
            .carrier
            .__verter_as_any()
            .downcast_ref::<T>()
    }

    /// The artifact's erased carrier payload, gated ONLY by adapter
    /// identity — the sole cross-crate entry point for a registered
    /// adapter's own opener function (`open_vue_carrier` /
    /// `open_svelte_carrier`) to reach its artifact's carrier. The final
    /// typed narrowing happens at the generic call site, which only ever
    /// requests the owning adapter's own concrete carrier type.
    pub(crate) fn erased_carrier_for_adapter(
        &self,
        adapter_id: &FrameworkAdapterId,
    ) -> Option<Arc<dyn CarrierParse>> {
        (self.adapter_id == *adapter_id).then(|| Arc::clone(&self.carrier.inner.carrier))
    }

    /// The artifact's carrier payload, for identity-preservation witness
    /// tests only.
    #[cfg(test)]
    pub(super) fn carrier_payload_for_tests(&self) -> &RegisteredCarrierPayload {
        &self.carrier
    }

    /// Rebind registered geometry to the current source-authority snapshot.
    #[doc(hidden)]
    pub fn __rehome_registered(
        &self,
        accepted: &AcceptedRegisteredCarrierSource,
        expected_parse_key: &verter_language::ParseKey,
    ) -> Result<Self, SyntaxReject> {
        use verter_language::{SourceSpaceDescriptor, SourceSpaceId};
        let source = accepted.source();
        let (syntax_profile, accepted_parse_key) = parse_identity_for_accepted(accepted)
            .ok_or_else(|| SyntaxReject::UnsupportedProfile {
                parse_key: Arc::new(expected_parse_key.clone()),
                syntax_profile: Arc::clone(&self.syntax_profile),
                reason: UnsupportedSyntaxProfileReason::FrontendMismatch,
            })?;
        let identity_matches = accepted_parse_key == *expected_parse_key
            && *self.parse_key == accepted_parse_key
            && *self.syntax_profile == syntax_profile
            && self.adapter_id == *accepted.grammar().adapter_id()
            && self.language_id == *accepted.grammar().language_id()
            && self.adapter_id
                == *source
                    .resolved_file_language()
                    .adapter_id()
                    .ok_or(())
                    .map_err(|()| SyntaxReject::UnsupportedProfile {
                        parse_key: Arc::new(accepted_parse_key.clone()),
                        syntax_profile: Arc::new(syntax_profile.clone()),
                        reason: UnsupportedSyntaxProfileReason::FrontendMismatch,
                    })?
            && self.language_id
                == *source
                    .resolved_file_language()
                    .carrier_language_id()
                    .ok_or(())
                    .map_err(|()| SyntaxReject::UnsupportedProfile {
                        parse_key: Arc::new(accepted_parse_key.clone()),
                        syntax_profile: Arc::new(syntax_profile.clone()),
                        reason: UnsupportedSyntaxProfileReason::FrontendMismatch,
                    })?
            && *self.carrier.adapter_id() == self.adapter_id
            && *self.carrier.language_id() == self.language_id;
        if !identity_matches {
            return Err(SyntaxReject::UnsupportedProfile {
                parse_key: Arc::new(accepted_parse_key),
                syntax_profile: Arc::new(syntax_profile),
                reason: UnsupportedSyntaxProfileReason::FrontendMismatch,
            });
        }
        let inventory = CarrierBlockInventory::new_registered(
            Arc::from([SourceSpaceDescriptor::registered(SourceSpaceId(0), source)]),
            Arc::new(self.common.inventory.normalized_names().clone()),
            Arc::from(self.common.inventory.blocks().to_vec()),
            Arc::new(self.common.inventory.markup().clone()),
            &[source],
        )
        .map_err(|error| SyntaxReject::InvalidCarrierGeometry {
            parse_key: Arc::clone(&self.parse_key),
            syntax_profile: Arc::clone(&self.syntax_profile),
            error: Arc::new(error),
        })?;
        let carrier_structure_hash = compute_carrier_structure_hash(&inventory);
        Ok(Self {
            adapter_id: self.adapter_id.clone(),
            language_id: self.language_id.clone(),
            epoch: self.epoch.clone(),
            parse_key: Arc::clone(&self.parse_key),
            syntax_profile: Arc::clone(&self.syntax_profile),
            common: FrameworkParseCommon {
                inventory: Arc::new(inventory),
                diagnostics: self.common.diagnostics.clone(),
            },
            carrier_structure_hash,
            carrier: self.carrier.clone(),
            _geometry: super::registered_geometry_state::RegisteredGeometry { _private: () },
        })
    }
}

fn parse_identity_for_accepted(
    accepted: &AcceptedRegisteredCarrierSource,
) -> Option<(verter_language::SyntaxProfileId, verter_language::ParseKey)> {
    let options = parse_options_for_accepted(accepted);
    let language = accepted.source().resolved_file_language();
    verter_language::parse_identity_for(accepted.source().bytes(), language, &options).ok()
}

fn parse_options_for_accepted(accepted: &AcceptedRegisteredCarrierSource) -> ParseOptions {
    match accepted.grammar().canonical_config() {
        CarrierGrammarConfig::Vue {
            delimiters,
            custom_elements,
        } => ParseOptions {
            delimiters: (
                delimiters.open().to_string(),
                delimiters.close().to_string(),
            ),
            custom_elements: custom_elements
                .iter()
                .map(|name| name.as_str().to_string())
                .collect(),
            svelte_loose: false,
        },
        // The registered grammar authority has no loose-mode concept yet —
        // every registered Svelte source requests strict parsing.
        CarrierGrammarConfig::Svelte => ParseOptions::default(),
    }
}

#[cfg(test)]
pub(crate) fn registered_artifact_for_tests(
    artifact: &Arc<UnregisteredFrameworkParseArtifact>,
    inventory: Arc<CarrierBlockInventory>,
    carrier: Arc<dyn CarrierParse>,
) -> Arc<FrameworkParseArtifact> {
    let carrier_structure_hash = compute_carrier_structure_hash(&inventory);
    let epoch = frontend_epoch(&artifact.adapter_id, &artifact.language_id)
        .unwrap_or_else(|| FrameworkEpochId::new(artifact.adapter_id.as_str()));
    Arc::new(FrameworkParseArtifact {
        adapter_id: artifact.adapter_id.clone(),
        language_id: artifact.language_id.clone(),
        epoch,
        parse_key: Arc::clone(&artifact.parse_key),
        syntax_profile: Arc::clone(&artifact.syntax_profile),
        common: FrameworkParseCommon {
            inventory,
            diagnostics: artifact.diagnostics.clone(),
        },
        carrier_structure_hash,
        carrier: RegisteredCarrierPayload::new(
            carrier,
            artifact.adapter_id.clone(),
            artifact.language_id.clone(),
        ),
        _geometry: super::registered_geometry_state::RegisteredGeometry { _private: () },
    })
}

#[cfg(test)]
pub(crate) fn parse_registered_source_for_tests(
    language: verter_language::FileLanguage,
    config: CarrierGrammarConfig,
    source: &str,
) -> Arc<FrameworkParseArtifact> {
    use verter_language::carrier_grammar::{
        CarrierGrammarAuthority, CarrierParserGrammarVersion, FrameworkAdapterSemanticVersion,
    };
    use verter_language::registered_source_authority::{
        CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
    };

    let source_authority = RegisteredSourceAuthority::new().expect("source authority");
    let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
    grammar_authority
        .register_carrier_grammar(
            language.clone(),
            FrameworkAdapterSemanticVersion::new(1).unwrap(),
            CarrierParserGrammarVersion::new(1).unwrap(),
            config.clone(),
        )
        .expect("register grammar");
    let snapshot = source_authority
        .register_source(
            CanonicalFileId::new("file:///fixture.carrier"),
            FileIncarnation::new(1),
            SourceGeneration::new(1),
            language,
            Arc::from(source),
        )
        .expect("register source");
    let accepted = grammar_authority
        .accept_registered_source(&source_authority, &snapshot, &config)
        .expect("accept source");
    Arc::new(
        super::registry::CarrierCompilerRegistry::built_in()
            .project_registered(&accepted)
            .expect("fixture source parses")
            .into_framework_parse_artifact(),
    )
}

impl std::fmt::Debug for FrameworkParseArtifact {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FrameworkParseArtifact")
            .field("adapter_id", &self.adapter_id)
            .field("language_id", &self.language_id)
            .field("epoch", &self.epoch)
            .field("parse_key", &self.parse_key)
            .field("common", &self.common)
            .finish_non_exhaustive()
    }
}

struct RegisteredCarrierPayloadInner {
    carrier: Arc<dyn CarrierParse>,
    adapter_id: FrameworkAdapterId,
    language_id: LanguageId,
}

impl RegisteredCarrierPayload {
    fn new(
        carrier: Arc<dyn CarrierParse>,
        adapter_id: FrameworkAdapterId,
        language_id: LanguageId,
    ) -> Self {
        Self {
            inner: Arc::new(RegisteredCarrierPayloadInner {
                carrier,
                adapter_id,
                language_id,
            }),
        }
    }

    /// Adapter that owns the retained parse.
    #[must_use]
    pub fn adapter_id(&self) -> &FrameworkAdapterId {
        &self.inner.adapter_id
    }

    /// Carrier language parsed by the owning adapter.
    #[must_use]
    pub fn language_id(&self) -> &LanguageId {
        &self.inner.language_id
    }

    /// Whether `self` and `other` retain the SAME inner Arc (identity, not
    /// structural equality) — the test-only witness that a value threaded
    /// through unchanged rather than being rebuilt.
    #[cfg(test)]
    pub(super) fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

#[doc(hidden)]
pub struct RegisteredCarrierProjection {
    carrier: RegisteredCarrierPayload,
    inventory: Arc<CarrierBlockInventory>,
    carrier_structure_hash: CarrierStructureHash,
    diagnostics: Vec<LanguageDiagnostic>,
    parse_key: Arc<verter_language::ParseKey>,
    syntax_profile: Arc<verter_language::SyntaxProfileId>,
    epoch: FrameworkEpochId,
}

impl RegisteredCarrierProjection {
    #[cfg(test)]
    pub(super) fn carrier(&self) -> &RegisteredCarrierPayload {
        &self.carrier
    }

    #[cfg(test)]
    pub(super) fn inventory(&self) -> &Arc<CarrierBlockInventory> {
        &self.inventory
    }

    /// Consume the exact projector result into the registered artifact.
    #[doc(hidden)]
    pub fn into_framework_parse_artifact(self) -> FrameworkParseArtifact {
        let Self {
            carrier,
            inventory,
            carrier_structure_hash,
            diagnostics,
            parse_key,
            syntax_profile,
            epoch,
        } = self;
        verter_debug_assert_eq!(
            compute_carrier_structure_hash(&inventory),
            carrier_structure_hash,
        );
        FrameworkParseArtifact {
            adapter_id: carrier.inner.adapter_id.clone(),
            language_id: carrier.inner.language_id.clone(),
            epoch,
            parse_key,
            syntax_profile,
            common: FrameworkParseCommon {
                inventory,
                diagnostics,
            },
            carrier_structure_hash,
            carrier,
            _geometry: super::registered_geometry_state::RegisteredGeometry { _private: () },
        }
    }
}

/// The registered compilers this crate knows how to project — a closed,
/// exhaustive dispatch set for the registered-projection path.
///
/// There is no external `&dyn CarrierCompiler` entry point into registered
/// projection: a bogus third-party `CarrierCompiler` implementation cannot
/// reach a projection arm at all, because it is not a variant of this
/// enum — the match below is exhaustive by construction, with no
/// wildcard arm and no `unreachable!()`.
#[derive(Clone)]
pub(super) enum KnownRegisteredCompiler {
    Vue(Arc<VueCarrierCompiler>),
    Svelte(Arc<SvelteCarrierCompiler>),
}

impl KnownRegisteredCompiler {
    pub(super) fn adapter_id(&self) -> FrameworkAdapterId {
        match self {
            Self::Vue(compiler) => compiler.adapter_id(),
            Self::Svelte(compiler) => compiler.adapter_id(),
        }
    }

    fn carrier_language_id(&self) -> LanguageId {
        match self {
            Self::Vue(compiler) => compiler.carrier_language_id(),
            Self::Svelte(compiler) => compiler.carrier_language_id(),
        }
    }
}

/// Installed Vue/Svelte frontend row stored in the immutable catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstalledCarrierFrontend {
    /// Vue SFC frontend.
    Vue(VueCarrierFrontend),
    /// Svelte component frontend.
    Svelte(SvelteCarrierFrontend),
}

/// Admission token for a catalog-selected Vue or Svelte frontend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstalledParseAdmission {
    /// Vue frontend admission.
    Vue(VueParseAdmission),
    /// Svelte frontend admission.
    Svelte(SvelteParseAdmission),
}

impl CarrierFrontend for InstalledCarrierFrontend {
    type ParseArtifact = Arc<UnregisteredFrameworkParseArtifact>;
    type SyntaxReject = SyntaxReject;
    type ParseAdmission = InstalledParseAdmission;

    fn parse(
        &self,
        source: &str,
        opts: &ParseOptions,
    ) -> Result<Arc<UnregisteredFrameworkParseArtifact>, SyntaxReject> {
        #[cfg(test)]
        REGISTERED_FRONTEND_PARSE_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match self {
            Self::Vue(frontend) => frontend.parse(source, opts),
            Self::Svelte(frontend) => frontend.parse(source, opts),
        }
    }
}

#[cfg(test)]
static REGISTERED_FRONTEND_PARSE_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
pub(super) fn registered_frontend_parse_count() -> usize {
    REGISTERED_FRONTEND_PARSE_COUNT.load(std::sync::atomic::Ordering::SeqCst)
}

/// Frozen Vue + Svelte frontend catalog. Built once from the Vue/Svelte
/// frontend registration constructors; no insert after.
#[must_use]
pub fn built_in_frontend_catalog(
) -> &'static ImmutableCapabilityCatalog<InstalledCarrierFrontend, (), (), (), ()> {
    static CATALOG: OnceLock<ImmutableCapabilityCatalog<InstalledCarrierFrontend, (), (), (), ()>> =
        OnceLock::new();
    CATALOG.get_or_init(|| {
        ImmutableCapabilityCatalog::try_from_rows([
            CatalogRow::from(
                vue_carrier_frontend_registration().map_frontend(InstalledCarrierFrontend::Vue),
            ),
            CatalogRow::from(
                svelte_carrier_frontend_registration()
                    .map_frontend(InstalledCarrierFrontend::Svelte),
            ),
        ])
        .expect("built-in Vue and Svelte frontend identities are unique")
    })
}

/// The unique frontend catalog row for one adapter × carrier-language
/// identity, or `None` when no frontend is registered for it.
fn frontend_row_for(
    adapter_id: &FrameworkAdapterId,
    carrier_language_id: &LanguageId,
) -> Option<&'static CatalogRow<InstalledCarrierFrontend, (), (), (), ()>> {
    built_in_frontend_catalog().iter().find(|row| {
        let identity = row.identity();
        identity.capability() == CatalogCapability::Frontend
            && identity.adapter_id() == adapter_id
            && identity.carrier_language_id() == carrier_language_id
    })
}

/// Catalog lookup for a registered Vue/Svelte frontend. Unknown adapter or
/// language returns `None` — no fallback parse.
#[must_use]
pub fn registered_frontend_for(
    adapter_id: &FrameworkAdapterId,
    carrier_language_id: &LanguageId,
) -> Option<&'static InstalledCarrierFrontend> {
    frontend_row_for(adapter_id, carrier_language_id).and_then(|row| row.frontend())
}

/// Catalog lookup for the registered carrier-grammar fact on a frontend
/// row. Unknown identity, or a frontend row without a registered grammar
/// fact, returns `None` — never another framework's grammar.
#[must_use]
pub fn registered_grammar_for(
    adapter_id: &FrameworkAdapterId,
    carrier_language_id: &LanguageId,
) -> Option<&'static CarrierGrammarConfig> {
    frontend_row_for(adapter_id, carrier_language_id).and_then(|row| row.registered_grammar())
}

fn frontend_epoch(
    adapter_id: &FrameworkAdapterId,
    carrier_language_id: &LanguageId,
) -> Option<FrameworkEpochId> {
    frontend_row_for(adapter_id, carrier_language_id).map(|row| row.identity().epoch().clone())
}

/// Template facts plus the diagnostics their extraction produced.
///
/// The template-fact producer is the ONLY pass that parses template
/// directive/interpolation expressions on the analysis route, so its
/// diagnostics (e.g. an `XInvalidExpression` for a malformed `v-if`
/// expression) are part of the product: a consumer that publishes the
/// facts publishes these diagnostics alongside them. Dropping them
/// silently erases the file's template expression errors.
#[derive(Debug, Default)]
pub struct TemplateFactsProduct {
    /// The extracted raw template data.
    pub data: RawTemplateData,
    /// Diagnostics emitted while extracting the facts (deduplicated by
    /// the consumer against its own parse-time channel).
    pub diagnostics: Vec<RuntimeDiagnostic>,
}

/// Type-erased semantic payload stored on a catalog row.
///
/// Lookup invokes [`Self::eval_source`] / [`Self::template_facts`]
/// directly. The generic selector has no Vue/Svelte match.
#[derive(Clone, Copy)]
pub struct InstalledSemanticAuthority {
    eval_source_fn: fn(&str, &FrameworkParseArtifact) -> Arc<str>,
    template_facts_fn: fn(&str, &FrameworkParseArtifact) -> Option<TemplateFactsProduct>,
}

impl PartialEq for InstalledSemanticAuthority {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::fn_addr_eq(self.eval_source_fn, other.eval_source_fn)
            && std::ptr::fn_addr_eq(self.template_facts_fn, other.template_facts_fn)
    }
}

impl Eq for InstalledSemanticAuthority {}

impl std::fmt::Debug for InstalledSemanticAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstalledSemanticAuthority")
            .finish_non_exhaustive()
    }
}

impl InstalledSemanticAuthority {
    const fn new(
        eval_source_fn: fn(&str, &FrameworkParseArtifact) -> Arc<str>,
        template_facts_fn: fn(&str, &FrameworkParseArtifact) -> Option<TemplateFactsProduct>,
    ) -> Self {
        Self {
            eval_source_fn,
            template_facts_fn,
        }
    }

    /// Position-preserving eval source from the catalog-selected row.
    #[must_use]
    pub fn eval_source(&self, source: &str, artifact: &FrameworkParseArtifact) -> Arc<str> {
        (self.eval_source_fn)(source, artifact)
    }

    /// Template facts from the catalog-selected row.
    ///
    /// `None` is producer failure or identity refusal, never fabricated
    /// empty success. A valid template-free carrier is `Some` empty facts.
    #[must_use]
    pub fn template_facts(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
    ) -> Option<TemplateFactsProduct> {
        #[cfg(any(test, feature = "test-support"))]
        TEMPLATE_FACTS_PRODUCER_INVOCATIONS.with(|count| count.set(count.get().saturating_add(1)));
        (self.template_facts_fn)(source, artifact)
    }
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Per-thread count of catalog semantic-producer executions.
    static TEMPLATE_FACTS_PRODUCER_INVOCATIONS: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0) };
}

/// Catalog semantic-producer executions on the calling thread since the
/// last take. Test observability only.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn take_template_facts_producer_invocations() -> u64 {
    TEMPLATE_FACTS_PRODUCER_INVOCATIONS.with(std::cell::Cell::take)
}

fn vue_semantic_eval_source(source: &str, artifact: &FrameworkParseArtifact) -> Arc<str> {
    FrameworkSemanticAuthority::<VueSfcV3>::eval_source(&VueSemanticAuthority, source, artifact)
}

fn vue_semantic_template_facts(
    source: &str,
    artifact: &FrameworkParseArtifact,
) -> Option<TemplateFactsProduct> {
    FrameworkSemanticAuthority::<VueSfcV3>::template_facts(&VueSemanticAuthority, source, artifact)
}

fn svelte_semantic_eval_source(source: &str, artifact: &FrameworkParseArtifact) -> Arc<str> {
    FrameworkSemanticAuthority::<SvelteSfc5>::eval_source(
        &SvelteSemanticAuthority,
        source,
        artifact,
    )
}

fn svelte_semantic_template_facts(
    source: &str,
    artifact: &FrameworkParseArtifact,
) -> Option<TemplateFactsProduct> {
    FrameworkSemanticAuthority::<SvelteSfc5>::template_facts(
        &SvelteSemanticAuthority,
        source,
        artifact,
    )
}

fn source_binds_to_artifact_parse_key(source: &str, artifact: &FrameworkParseArtifact) -> bool {
    let language = FileLanguage::Framework {
        adapter_id: artifact.adapter_id().clone(),
        language_id: artifact.language_id().clone(),
    };
    let (domain, epoch) = if language.is_vue() {
        (
            VUE_SYNTAX_COMPATIBILITY_DOMAIN,
            VUE_SYNTAX_COMPATIBILITY_EPOCH,
        )
    } else if language.is_svelte() {
        (
            SVELTE_SYNTAX_COMPATIBILITY_DOMAIN,
            SVELTE_SYNTAX_COMPATIBILITY_EPOCH,
        )
    } else {
        (
            FRAMEWORK_SYNTAX_COMPATIBILITY_DOMAIN,
            FRAMEWORK_SYNTAX_COMPATIBILITY_EPOCH,
        )
    };
    match parse_key_for(source, &language, domain, epoch, artifact.syntax_profile()) {
        Ok(key) => key == *artifact.parse_key(),
        Err(_) => false,
    }
}

/// Frozen Vue + Svelte semantic catalog. Built once from the Vue/Svelte
/// semantic registration constructors; no insert after.
#[must_use]
pub fn built_in_semantic_catalog(
) -> &'static ImmutableCapabilityCatalog<(), (), InstalledSemanticAuthority, (), ()> {
    static CATALOG: OnceLock<
        ImmutableCapabilityCatalog<(), (), InstalledSemanticAuthority, (), ()>,
    > = OnceLock::new();
    CATALOG.get_or_init(|| {
        ImmutableCapabilityCatalog::try_from_rows([
            CatalogRow::from(vue_semantic_authority_registration().map_semantic(|_| {
                InstalledSemanticAuthority::new(
                    vue_semantic_eval_source,
                    vue_semantic_template_facts,
                )
            })),
            CatalogRow::from(svelte_semantic_authority_registration().map_semantic(|_| {
                InstalledSemanticAuthority::new(
                    svelte_semantic_eval_source,
                    svelte_semantic_template_facts,
                )
            })),
        ])
        .expect("built-in Vue and Svelte semantic identities are unique")
    })
}

/// Catalog lookup for a registered semantic authority by adapter × epoch.
/// Unknown or mismatched identity returns `None` — no framework fallback.
#[must_use]
pub fn registered_semantic_for(
    adapter_id: &FrameworkAdapterId,
    epoch: &FrameworkEpochId,
) -> Option<&'static InstalledSemanticAuthority> {
    built_in_semantic_catalog().iter().find_map(|row| {
        let identity = row.identity();
        (identity.capability() == CatalogCapability::Semantic
            && identity.adapter_id() == adapter_id
            && identity.epoch() == epoch)
            .then(|| row.semantic())
            .flatten()
    })
}

/// Identity-bound semantic catalog row: adapter × registered frontend
/// epoch × Semantic. No artifact, no parse, no Vue/Svelte match.
#[must_use]
pub fn registered_semantic_for_frontend(
    adapter_id: &FrameworkAdapterId,
    carrier_language_id: &LanguageId,
) -> Option<&'static InstalledSemanticAuthority> {
    let epoch = frontend_epoch(adapter_id, carrier_language_id)?;
    registered_semantic_for(adapter_id, &epoch)
}

/// One `built_in_semantic_catalog` lookup keyed adapter × artifact epoch
/// × Semantic, then the selected row's eval-source payload. Catalog miss
/// is `None` — no frontend hop, no Vue/Svelte match, no blanking.
#[must_use]
pub fn eval_source_from_catalog(
    artifact: &FrameworkParseArtifact,
    source: &str,
) -> Option<Arc<str>> {
    registered_semantic_for(artifact.adapter_id(), artifact.epoch())
        .map(|authority| authority.eval_source(source, artifact))
}

/// Which template bytes a catalog template-fact query is bound to.
///
/// Selected content is not parse admission: it binds only when those
/// bytes equal the unique admitted template-host region. Catalog
/// extraction always uses the original carrier source and artifact so
/// spans stay SFC-absolute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateFactsBasis<'a> {
    /// The registered artifact already admits the template region.
    AdmittedArtifact,
    /// Bind only when these bytes equal the unique admitted TemplateHost.
    SelectedTemplate(&'a str),
}

/// One `built_in_semantic_catalog` lookup keyed adapter × artifact epoch
/// × Semantic, then the selected row's template-fact payload. Catalog
/// miss, parse-key mismatch, selected-template mismatch, and producer
/// failure are `None` — no frontend hop, no Vue/Svelte match, no empty
/// success.
#[must_use]
pub fn template_facts_from_catalog(
    artifact: &FrameworkParseArtifact,
    source: &str,
    basis: TemplateFactsBasis<'_>,
) -> Option<TemplateFactsProduct> {
    if let TemplateFactsBasis::SelectedTemplate(bytes) = basis {
        if !selected_template_equals_admitted_host(artifact, bytes) {
            return None;
        }
    }
    if !source_binds_to_artifact_parse_key(source, artifact) {
        return None;
    }
    registered_semantic_for(artifact.adapter_id(), artifact.epoch())?
        .template_facts(source, artifact)
}

fn selected_template_equals_admitted_host(artifact: &FrameworkParseArtifact, bytes: &str) -> bool {
    let mut host = None;
    for block in artifact.inventory().blocks() {
        let CarrierBlock::Section {
            role: SectionRole::TemplateHost,
            syntax,
            ..
        } = block
        else {
            continue;
        };
        match host {
            Some(_) => return false,
            None => host = Some(syntax),
        }
    }
    let Some(syntax) = host else {
        return false;
    };
    matches!(
        artifact.inventory().slice_span(syntax.content_span),
        Ok(admitted) if admitted == bytes
    )
}

/// Execution inputs excluded from projection-request identity.
#[derive(Debug, Clone, Default)]
pub struct ProjectionCatalogInputs {
    /// Host-selected block bytes for multi-unit IDE composition.
    pub block_content: RuntimeBlockContentInputs,
    /// Resolved Vue facts threaded beside the request.
    pub vue_execution: VueExecutionInputs,
    /// Authoritative Vue macro semantics, when supplied.
    pub vue_macros: VueMacroSemanticInput,
}

/// IDE companion plus compile diagnostics from a catalog-selected backend.
#[derive(Debug, Clone)]
pub struct InstalledIdeCompanion {
    /// Generated TSX/JSX companion.
    pub ide: super::carrier_compiler::IdeOutput,
    /// Non-fatal compile diagnostics tagged by the selected backend.
    pub diagnostics: Vec<RuntimeDiagnostic>,
}

/// Type-erased projection payload stored on a catalog row.
///
/// Lookup invokes [`Self::project_ide`] directly. The generic selector has
/// no Vue/Svelte match.
#[derive(Clone, Copy)]
pub struct InstalledProjectionBackend {
    project_ide_fn: fn(
        super::capability::ProductExecutionGrant,
        &str,
        &FrameworkParseArtifact,
        &CompileRequest,
        &ProjectionCatalogInputs,
    ) -> Result<InstalledIdeCompanion, CompileUnsupported>,
}

impl PartialEq for InstalledProjectionBackend {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::fn_addr_eq(self.project_ide_fn, other.project_ide_fn)
    }
}

impl Eq for InstalledProjectionBackend {}

impl std::fmt::Debug for InstalledProjectionBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstalledProjectionBackend")
            .finish_non_exhaustive()
    }
}

impl InstalledProjectionBackend {
    const fn new(
        project_ide_fn: fn(
            super::capability::ProductExecutionGrant,
            &str,
            &FrameworkParseArtifact,
            &CompileRequest,
            &ProjectionCatalogInputs,
        ) -> Result<InstalledIdeCompanion, CompileUnsupported>,
    ) -> Self {
        Self { project_ide_fn }
    }

    /// Project the IDE companion from the catalog-selected row. Consumes
    /// the projection leg's execution grant by value.
    pub fn project_ide(
        &self,
        grant: super::capability::ProductExecutionGrant,
        source: &str,
        artifact: &FrameworkParseArtifact,
        request: &CompileRequest,
        inputs: &ProjectionCatalogInputs,
    ) -> Result<InstalledIdeCompanion, CompileUnsupported> {
        #[cfg(any(test, feature = "test-support"))]
        PROJECTION_PRODUCER_INVOCATIONS.with(|count| count.set(count.get().saturating_add(1)));
        (self.project_ide_fn)(grant, source, artifact, request, inputs)
    }
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Per-thread count of catalog projection-producer executions.
    static PROJECTION_PRODUCER_INVOCATIONS: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
thread_local! {
    /// Per-thread count of projection catalog CONSULTS (row lookups), so
    /// tests can prove which demand validations touch the projection
    /// capability at all. Counted at the one lookup choke point
    /// ([`registered_projection_for`]), not at call sites, so any future
    /// consult path is counted too.
    static PROJECTION_CATALOG_CONSULTS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Projection catalog consults on the calling thread. Test observability
/// only.
#[cfg(test)]
#[must_use]
pub(super) fn projection_catalog_consult_count() -> u64 {
    PROJECTION_CATALOG_CONSULTS.with(std::cell::Cell::get)
}

/// Catalog projection-producer executions on the calling thread since the
/// last take. Test observability only.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn take_projection_producer_invocations() -> u64 {
    PROJECTION_PRODUCER_INVOCATIONS.with(std::cell::Cell::take)
}

fn vue_project_ide(
    grant: super::capability::ProductExecutionGrant,
    source: &str,
    artifact: &FrameworkParseArtifact,
    request: &CompileRequest,
    inputs: &ProjectionCatalogInputs,
) -> Result<InstalledIdeCompanion, CompileUnsupported> {
    match VueProjectionBackend.project_ide(
        grant,
        source,
        artifact,
        request,
        &VueProjectionInputs {
            block_content: inputs.block_content.clone(),
            execution: inputs.vue_execution.clone(),
            macros: inputs.vue_macros.clone(),
        },
    ) {
        Ok(companion) => Ok(InstalledIdeCompanion {
            ide: companion.ide,
            diagnostics: companion
                .diagnostics
                .into_iter()
                .map(|tagged| RuntimeDiagnostic {
                    severity: tagged.diagnostic.severity.into(),
                    code: tagged.diagnostic.code,
                    message: tagged.diagnostic.message,
                    span: tagged
                        .diagnostic
                        .span
                        .unwrap_or_else(|| verter_span::Span::new(0, source.len() as u32)),
                })
                .collect(),
        }),
        Err(error) => Err(map_vue_projection_error(error)),
    }
}

fn svelte_project_ide(
    grant: super::capability::ProductExecutionGrant,
    source: &str,
    artifact: &FrameworkParseArtifact,
    request: &CompileRequest,
    _inputs: &ProjectionCatalogInputs,
) -> Result<InstalledIdeCompanion, CompileUnsupported> {
    match SvelteProjectionBackend.project_ide(
        grant,
        source,
        artifact,
        request,
        &SvelteProjectionInputs,
    ) {
        Ok(companion) => Ok(InstalledIdeCompanion {
            ide: companion.ide,
            diagnostics: companion
                .diagnostics
                .into_iter()
                .map(|tagged| RuntimeDiagnostic {
                    severity: match tagged.diagnostic.severity {
                        crate::svelte::ide::DiagnosticSeverity::Error => {
                            RuntimeDiagnosticSeverity::Error
                        }
                        crate::svelte::ide::DiagnosticSeverity::Information => {
                            RuntimeDiagnosticSeverity::Info
                        }
                    },
                    code: tagged.diagnostic.code.to_string(),
                    message: tagged.diagnostic.message,
                    span: tagged.diagnostic.span,
                })
                .collect(),
        }),
        Err(error) => Err(map_svelte_projection_error(error)),
    }
}

fn map_vue_projection_error(error: VueProjectionError) -> CompileUnsupported {
    match error {
        VueProjectionError::Unsupported(unsupported) => unsupported,
        VueProjectionError::NotIdeOnly { .. } => CompileUnsupported::TargetMissingIde,
        VueProjectionError::Direct(DirectCompileError::Vue(error)) => {
            CompileUnsupported::RequestExecutionRefused(error)
        }
        VueProjectionError::Direct(_) => CompileUnsupported::TargetMissingIde,
    }
}

fn map_svelte_projection_error(error: SvelteProjectionError) -> CompileUnsupported {
    match error {
        SvelteProjectionError::Unsupported(unsupported) => unsupported,
        SvelteProjectionError::NotIdeOnly { .. } => CompileUnsupported::TargetMissingIde,
        SvelteProjectionError::Direct(_) => CompileUnsupported::TargetMissingIde,
    }
}

/// Frozen Vue + Svelte projection catalog. Built once from the Vue/Svelte
/// projection registration constructors; no insert after.
#[must_use]
pub fn built_in_projection_catalog(
) -> &'static ImmutableCapabilityCatalog<(), InstalledProjectionBackend, (), (), ()> {
    static CATALOG: OnceLock<
        ImmutableCapabilityCatalog<(), InstalledProjectionBackend, (), (), ()>,
    > = OnceLock::new();
    CATALOG.get_or_init(|| {
        ImmutableCapabilityCatalog::try_from_rows([
            CatalogRow::from(
                vue_projection_backend_registration()
                    .map_projection(|_| InstalledProjectionBackend::new(vue_project_ide)),
            ),
            CatalogRow::from(
                svelte_projection_backend_registration()
                    .map_projection(|_| InstalledProjectionBackend::new(svelte_project_ide)),
            ),
        ])
        .expect("built-in Vue and Svelte projection identities are unique")
    })
}

/// Catalog lookup for a registered projection backend by adapter × epoch.
/// Unknown or mismatched identity returns `None` — no framework fallback.
#[must_use]
pub fn registered_projection_for(
    adapter_id: &FrameworkAdapterId,
    epoch: &FrameworkEpochId,
) -> Option<&'static InstalledProjectionBackend> {
    #[cfg(test)]
    PROJECTION_CATALOG_CONSULTS.with(|count| count.set(count.get().saturating_add(1)));
    built_in_projection_catalog().iter().find_map(|row| {
        let identity = row.identity();
        (identity.capability() == CatalogCapability::Projection
            && identity.adapter_id() == adapter_id
            && identity.epoch() == epoch)
            .then(|| row.projection())
            .flatten()
    })
}

/// One `built_in_projection_catalog` lookup keyed adapter × artifact epoch
/// × Projection, then the selected row's IDE payload. Catalog miss is
/// [`CompileUnsupported::NoIdeProjection`] — no frontend hop, no Vue/Svelte
/// match, no silent empty companion.
pub fn project_ide_from_catalog(
    grant: super::capability::ProductExecutionGrant,
    artifact: &FrameworkParseArtifact,
    source: &str,
    request: &CompileRequest,
    inputs: &ProjectionCatalogInputs,
) -> Result<InstalledIdeCompanion, CompileUnsupported> {
    registered_projection_for(artifact.adapter_id(), artifact.epoch())
        .ok_or_else(|| CompileUnsupported::NoIdeProjection {
            adapter_id: artifact.adapter_id().clone(),
        })?
        .project_ide(grant, source, artifact, request, inputs)
}

fn catalog_miss_reject(
    source: &str,
    adapter_id: &FrameworkAdapterId,
    carrier_language_id: &LanguageId,
    opts: &ParseOptions,
) -> SyntaxReject {
    let language = verter_language::FileLanguage::Framework {
        adapter_id: adapter_id.clone(),
        language_id: carrier_language_id.clone(),
    };
    let (syntax_profile, parse_key) = verter_language::parse_identity_for(source, &language, opts)
        .expect("requested adapter/language/options identity is constructible without Vue/Svelte substitution");
    SyntaxReject::UnsupportedProfile {
        parse_key: Arc::new(parse_key),
        syntax_profile: Arc::new(syntax_profile),
        reason: UnsupportedSyntaxProfileReason::FrontendMismatch,
    }
}

fn catalog_miss_from_accepted(accepted: &AcceptedRegisteredCarrierSource) -> SyntaxReject {
    parse_identity_for_accepted(accepted)
        .map(
            |(syntax_profile, parse_key)| SyntaxReject::UnsupportedProfile {
                parse_key: Arc::new(parse_key),
                syntax_profile: Arc::new(syntax_profile),
                reason: UnsupportedSyntaxProfileReason::FrontendMismatch,
            },
        )
        .unwrap_or_else(|| {
            let language = accepted.source().resolved_file_language();
            catalog_miss_reject(
                accepted.source().bytes(),
                &language
                    .adapter_id()
                    .cloned()
                    .unwrap_or_else(|| FrameworkAdapterId::new("")),
                &language
                    .carrier_language_id()
                    .cloned()
                    .unwrap_or_else(|| LanguageId::new("")),
                &parse_options_for_accepted(accepted),
            )
        })
}

/// Project an accepted registered source: catalog frontend parse, then
/// registered geometry. Callers never look up [`super::CarrierCompilerRegistry`].
/// A catalog miss is [`SyntaxReject::UnsupportedProfile`] (`FrontendMismatch`).
pub fn project_registered_accepted(
    accepted: &AcceptedRegisteredCarrierSource,
) -> Result<RegisteredCarrierProjection, SyntaxReject> {
    let language = accepted.source().resolved_file_language();
    let adapter_id = language
        .adapter_id()
        .ok_or_else(|| catalog_miss_from_accepted(accepted))?;
    let carrier_language_id = language
        .carrier_language_id()
        .ok_or_else(|| catalog_miss_from_accepted(accepted))?;
    let frontend = registered_frontend_for(adapter_id, carrier_language_id)
        .ok_or_else(|| catalog_miss_from_accepted(accepted))?;
    let known = match frontend {
        InstalledCarrierFrontend::Vue(_) => {
            KnownRegisteredCompiler::Vue(Arc::new(VueCarrierCompiler))
        }
        InstalledCarrierFrontend::Svelte(_) => {
            KnownRegisteredCompiler::Svelte(Arc::new(SvelteCarrierCompiler))
        }
    };
    project_registered_carrier(Some(&known), accepted)
}

/// Parse through the catalog frontend for `(adapter, language)`. A catalog
/// miss is [`SyntaxReject::UnsupportedProfile`] (`FrontendMismatch`) — never
/// a fallback parse and never a panic.
pub fn parse_registered_frontend(
    adapter_id: &FrameworkAdapterId,
    carrier_language_id: &LanguageId,
    source: &str,
    opts: &ParseOptions,
) -> Result<Arc<UnregisteredFrameworkParseArtifact>, SyntaxReject> {
    match registered_frontend_for(adapter_id, carrier_language_id) {
        Some(frontend) => frontend.parse(source, opts),
        None => Err(catalog_miss_reject(
            source,
            adapter_id,
            carrier_language_id,
            opts,
        )),
    }
}

/// The registered projection entry, dispatched over the closed compiler
/// enum. Its sole cross-crate caller is
/// [`CarrierCompilerRegistry::project_registered`](super::registry::CarrierCompilerRegistry::project_registered).
///
/// `Err(SyntaxReject)` means the carrier frontend refused the request before
/// producing an artifact — no geometry or publishable diagnostic product exists.
pub(super) fn project_registered_carrier(
    known: Option<&KnownRegisteredCompiler>,
    accepted: &AcceptedRegisteredCarrierSource,
) -> Result<RegisteredCarrierProjection, SyntaxReject> {
    let language = accepted.source().resolved_file_language();
    let adapter_id = language
        .adapter_id()
        .ok_or_else(|| catalog_miss_from_accepted(accepted))?;
    let carrier_language_id = language
        .carrier_language_id()
        .ok_or_else(|| catalog_miss_from_accepted(accepted))?;
    let known = known.ok_or_else(|| catalog_miss_from_accepted(accepted))?;
    if known.adapter_id() != *adapter_id || known.carrier_language_id() != *carrier_language_id {
        return Err(catalog_miss_from_accepted(accepted));
    }
    let options = parse_options_for_accepted(accepted);
    let artifact = parse_registered_frontend(
        adapter_id,
        carrier_language_id,
        accepted.source().bytes(),
        &options,
    )?;
    assert_eq!(artifact.adapter_id, known.adapter_id());
    assert_eq!(artifact.language_id, known.carrier_language_id());
    let diagnostics = artifact.diagnostics.clone();
    let parse_key = Arc::clone(&artifact.parse_key);
    let syntax_profile = Arc::clone(&artifact.syntax_profile);
    let (inventory, parsed_carrier): (CarrierBlockInventory, Arc<dyn CarrierParse>) = match known {
        KnownRegisteredCompiler::Vue(vue) => (
            project_vue(vue, accepted, &artifact).map_err(|error| {
                SyntaxReject::InvalidCarrierGeometry {
                    parse_key: Arc::clone(&parse_key),
                    syntax_profile: Arc::clone(&syntax_profile),
                    error: Arc::new(error),
                }
            })?,
            vue.unregistered_carrier_arc(&artifact)
                .expect("Vue carrier payload"),
        ),
        KnownRegisteredCompiler::Svelte(svelte) => (
            project_svelte(svelte, accepted, &artifact).map_err(|error| {
                SyntaxReject::InvalidCarrierGeometry {
                    parse_key: Arc::clone(&parse_key),
                    syntax_profile: Arc::clone(&syntax_profile),
                    error: Arc::new(error),
                }
            })?,
            svelte
                .unregistered_carrier_arc(&artifact)
                .expect("Svelte carrier payload"),
        ),
    };
    let inventory = Arc::new(inventory);
    let carrier_structure_hash = compute_carrier_structure_hash(&inventory);
    let carrier = RegisteredCarrierPayload::new(
        parsed_carrier,
        artifact.adapter_id.clone(),
        artifact.language_id.clone(),
    );
    let epoch = frontend_epoch(adapter_id, carrier_language_id)
        .ok_or_else(|| catalog_miss_from_accepted(accepted))?;
    Ok(RegisteredCarrierProjection {
        carrier,
        inventory,
        carrier_structure_hash,
        diagnostics,
        parse_key,
        syntax_profile,
        epoch,
    })
}

struct Builder<'a> {
    source: &'a str,
    names: Vec<Arc<str>>,
    name_ids: HashMap<Arc<str>, InternedNameId>,
    attributes: u32,
    nodes: Vec<MarkupSyntaxNode>,
    child_ids: Vec<MarkupNodeId>,
    roots: Vec<MarkupNodeId>,
}

impl<'a> Builder<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            names: Vec::new(),
            name_ids: HashMap::new(),
            attributes: 0,
            nodes: Vec::new(),
            child_ids: Vec::new(),
            roots: Vec::new(),
        }
    }
    fn span(&self, span: Span) -> SourceSpan {
        SourceSpan::new(SourceSpaceId(0), span.start, span.end)
    }
    fn raw_span(&self, start: u32, end: u32) -> SourceSpan {
        SourceSpan::new(SourceSpaceId(0), start, end)
    }
    fn slice(&self, span: Span) -> SourceSlice {
        SourceSlice::new(self.span(span))
    }
    fn intern(&mut self, value: &str) -> InternedNameId {
        if let Some(id) = self.name_ids.get(value) {
            return *id;
        }
        let value: Arc<str> = Arc::from(value);
        let id = InternedNameId(self.names.len() as u32);
        self.names.push(Arc::clone(&value));
        self.name_ids.insert(value, id);
        id
    }
    fn attribute_id(&mut self) -> AttributeId {
        let id = AttributeId(self.attributes);
        self.attributes += 1;
        id
    }
    fn finish(
        self,
        accepted: &AcceptedRegisteredCarrierSource,
        blocks: Vec<CarrierBlock>,
    ) -> Result<CarrierBlockInventory, InventoryValidationError> {
        CarrierBlockInventory::new_registered(
            Arc::from([SourceSpaceDescriptor::registered(
                SourceSpaceId(0),
                accepted.source(),
            )]),
            Arc::new(NormalizedNameTable {
                values: Arc::from(self.names),
            }),
            Arc::from(blocks),
            Arc::new(MarkupSyntaxArena {
                roots: Arc::from(self.roots),
                nodes: Arc::from(self.nodes),
                child_ids: Arc::from(self.child_ids),
            }),
            &[accepted.source()],
        )
    }
    fn add_node(
        &mut self,
        root_block: BlockId,
        parent: Option<MarkupNodeId>,
        kind: MarkupNodeKind,
        children: Vec<MarkupNodeId>,
    ) -> MarkupNodeId {
        let start = self.child_ids.len() as u32;
        self.child_ids.extend(children);
        let end = self.child_ids.len() as u32;
        let id = MarkupNodeId(self.nodes.len() as u32);
        self.nodes.push(MarkupSyntaxNode {
            id,
            root_block,
            parent,
            children: start..end,
            kind,
        });
        id
    }
}

fn project_vue(
    vue: &VueCarrierCompiler,
    accepted: &AcceptedRegisteredCarrierSource,
    artifact: &UnregisteredFrameworkParseArtifact,
) -> Result<CarrierBlockInventory, InventoryValidationError> {
    use verter_parser::parser::types::RootNodeKind;
    let parsed = vue.unregistered_parsed_sfc(artifact).expect("Vue artifact");
    enum Root<'a> {
        Script(&'a verter_parser::parser::types::RootNodeScript),
        Template(&'a verter_parser::ast::types::TemplateAst),
        Style(&'a verter_parser::parser::types::RootNodeStyle),
        Custom(&'a verter_parser::parser::types::RootNodeUnknown),
    }
    impl Root<'_> {
        fn start(&self) -> u32 {
            match self {
                Self::Script(v) => v.tag_open.start,
                Self::Template(v) => v.root.tag_open.start,
                Self::Style(v) => v.tag_open.start,
                Self::Custom(v) => v.tag_open.start,
            }
        }
    }
    let mut roots = Vec::new();
    roots.extend(parsed.script().map(Root::Script));
    roots.extend(parsed.script_setup().map(Root::Script));
    roots.extend(parsed.template_ast().map(Root::Template));
    roots.extend(parsed.style_nodes().iter().map(Root::Style));
    roots.extend(parsed.unknown_nodes().iter().map(Root::Custom));
    roots.sort_by_key(Root::start);
    let mut builder = Builder::new(accepted.source().bytes());
    let mut blocks = Vec::new();
    for root in roots {
        let id = BlockId(blocks.len() as u32);
        match root {
            Root::Script(v) => {
                let dialect = ScriptSourceType::from(super::vue_bridge::vue_script_source_type(
                    parsed,
                    accepted.source().bytes(),
                ));
                let role = if v.is_setup {
                    ScriptRole::Setup
                } else {
                    ScriptRole::Module
                };
                let content = v.content.map(|span| builder.span(span));
                let closing = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start, tag.end));
                let closing_name = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start + 2, tag.name_end));
                let syntax = vue_tagged(
                    &mut builder,
                    "script",
                    VueTaggedSpans {
                        start: v.tag_open.start,
                        name_end: v.tag_open.name_end,
                        open_end: v.tag_open.end,
                        content,
                        close: closing,
                        close_name: closing_name,
                    },
                    &v.attributes,
                );
                blocks.push(CarrierBlock::Section {
                    id,
                    role: SectionRole::Script { role, dialect },
                    syntax,
                });
            }
            Root::Template(ast) => {
                let v = &ast.root;
                let content = v.content.as_ref().map(|c| builder.raw_span(c.start, c.end));
                let closing = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start, tag.end));
                let closing_name = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start + 2, tag.name_end));
                let syntax = vue_tagged(
                    &mut builder,
                    "template",
                    VueTaggedSpans {
                        start: v.tag_open.start,
                        name_end: v.tag_open.name_end,
                        open_end: v.tag_open.end,
                        content,
                        close: closing,
                        close_name: closing_name,
                    },
                    &v.attributes,
                );
                blocks.push(CarrierBlock::Section {
                    id,
                    role: SectionRole::TemplateHost,
                    syntax,
                });
                if let Some(content) = &v.content {
                    for child in &content.children {
                        let node = project_vue_node(
                            &mut builder,
                            ast,
                            *child,
                            id,
                            None,
                            MarkupNamespace::Html,
                        );
                        builder.roots.push(node);
                    }
                }
            }
            Root::Style(v) => {
                let dialect = match v.lang {
                    Some(verter_parser::parser::types::StyleLang::Css) => StyleDialect::Css,
                    Some(verter_parser::parser::types::StyleLang::Scss) => StyleDialect::Scss,
                    Some(verter_parser::parser::types::StyleLang::Sass) => StyleDialect::Sass,
                    Some(verter_parser::parser::types::StyleLang::Less) => StyleDialect::Less,
                    Some(verter_parser::parser::types::StyleLang::Stylus) => StyleDialect::Stylus,
                    Some(verter_parser::parser::types::StyleLang::Unknown) => StyleDialect::Missing,
                    None => StyleDialect::Css,
                };
                let content = v.content.map(|span| builder.span(span));
                let closing = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start, tag.end));
                let closing_name = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start + 2, tag.name_end));
                let syntax = vue_tagged(
                    &mut builder,
                    "style",
                    VueTaggedSpans {
                        start: v.tag_open.start,
                        name_end: v.tag_open.name_end,
                        open_end: v.tag_open.end,
                        content,
                        close: closing,
                        close_name: closing_name,
                    },
                    &v.attributes,
                );
                blocks.push(CarrierBlock::Section {
                    id,
                    role: SectionRole::Style {
                        dialect,
                        scoped: v.scoped,
                        module: if v.module {
                            StyleModule::Default
                        } else {
                            StyleModule::None
                        },
                    },
                    syntax,
                });
            }
            Root::Custom(v) => {
                let name =
                    &builder.source[v.tag_open.start as usize + 1..v.tag_open.name_end as usize];
                let normalized = name.to_ascii_lowercase();
                let content = v.content.map(|span| builder.span(span));
                let closing = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start, tag.end));
                let closing_name = v
                    .tag_close
                    .as_ref()
                    .map(|tag| builder.raw_span(tag.start + 2, tag.name_end));
                let syntax = vue_tagged(
                    &mut builder,
                    name,
                    VueTaggedSpans {
                        start: v.tag_open.start,
                        name_end: v.tag_open.name_end,
                        open_end: v.tag_open.end,
                        content,
                        close: closing,
                        close_name: closing_name,
                    },
                    &v.attributes,
                );
                blocks.push(CarrierBlock::Section {
                    id,
                    role: SectionRole::Custom {
                        normalized_name: Arc::from(normalized),
                    },
                    syntax,
                });
            }
        }
    }
    let _ = RootNodeKind::Unknown;
    builder.finish(accepted, blocks)
}

struct VueTaggedSpans {
    start: u32,
    name_end: u32,
    open_end: u32,
    content: Option<SourceSpan>,
    close: Option<SourceSpan>,
    close_name: Option<SourceSpan>,
}

fn vue_tagged(
    builder: &mut Builder<'_>,
    name: &str,
    spans: VueTaggedSpans,
    props: &[verter_parser::types::NodeProp],
) -> TaggedSyntax {
    let VueTaggedSpans {
        start,
        name_end,
        open_end,
        content,
        close,
        close_name,
    } = spans;
    let name_span = builder.raw_span(start + 1, name_end);
    let normalized = builder.intern(&name.to_ascii_lowercase());
    let attributes = vue_attributes(builder, props);
    let content = content.unwrap_or(builder.raw_span(open_end, open_end));
    let full_end = close.map(|s| s.end).unwrap_or(content.end);
    TaggedSyntax {
        authored_name: SourceSlice::new(name_span),
        normalized_name: normalized,
        opening_span: builder.raw_span(start, open_end),
        opening_name_span: name_span,
        attribute_insertion_anchor: builder
            .raw_span(open_end.saturating_sub(1), open_end.saturating_sub(1)),
        content_span: content,
        closing_span: close,
        closing_name_span: close_name,
        full_span: builder.raw_span(start, full_end),
        termination: if close.is_some() {
            SyntaxTermination::Closed
        } else {
            SyntaxTermination::UnclosedEof
        },
        attributes: Arc::from(attributes),
    }
}

fn project_vue_node(
    builder: &mut Builder<'_>,
    ast: &verter_parser::ast::types::TemplateAst,
    id: verter_parser::types::NodeId,
    root_block: BlockId,
    parent: Option<MarkupNodeId>,
    parent_namespace: MarkupNamespace,
) -> MarkupNodeId {
    use verter_parser::ast::types::AstNodeKind;
    let placeholder = MarkupNodeId(builder.nodes.len() as u32);
    builder.nodes.push(MarkupSyntaxNode {
        id: placeholder,
        root_block,
        parent,
        children: 0..0,
        kind: MarkupNodeKind::Text {
            content_span: builder.raw_span(0, 0),
        },
    });
    let node = &ast.nodes[id.0];
    let (kind, children) = match &node.kind {
        AstNodeKind::Text(v) => (
            MarkupNodeKind::Text {
                content_span: builder.raw_span(v.start, v.end),
            },
            vec![],
        ),
        AstNodeKind::Comment(v) => (
            MarkupNodeKind::Comment {
                opening_span: builder.raw_span(v.start, v.content_start),
                content_span: builder.raw_span(v.content_start, v.content_end),
                closing_span: (v.content_end < v.end)
                    .then(|| builder.raw_span(v.content_end, v.end)),
                full_span: builder.raw_span(v.start, v.end),
                termination: if v.content_end < v.end {
                    SyntaxTermination::Closed
                } else {
                    SyntaxTermination::UnclosedEof
                },
            },
            vec![],
        ),
        AstNodeKind::Interpolation(v) => (
            MarkupNodeKind::Interpolation {
                family: MarkupInterpolationFamily::VueInterpolation,
                opening_span: builder.raw_span(v.start, v.inner_start),
                expression_span: builder.raw_span(v.inner_start, v.inner_end),
                closing_span: (v.inner_end < v.end).then(|| builder.raw_span(v.inner_end, v.end)),
                full_span: builder.raw_span(v.start, v.end),
                termination: if v.inner_end < v.end {
                    SyntaxTermination::Closed
                } else {
                    SyntaxTermination::UnclosedEof
                },
            },
            vec![],
        ),
        AstNodeKind::Element(v) => {
            let name_span = builder.raw_span(v.tag_open.start + 1, v.tag_open.name_end);
            let name = builder.source[name_span.start as usize..name_span.end as usize].to_string();
            let lower_name = name.to_ascii_lowercase();
            let namespace = match lower_name.as_str() {
                "svg" => MarkupNamespace::Svg,
                "math" => MarkupNamespace::MathMl,
                _ => parent_namespace,
            };
            let parser_known_native =
                verter_parser::utils::vue::tag::is_html_tag(lower_name.as_bytes())
                    || verter_parser::utils::vue::tag::is_svg_tag(lower_name.as_bytes())
                    || verter_parser::utils::vue::tag::is_mathml_tag(lower_name.as_bytes());
            let normalized = if v.tag_type.is_component() && !parser_known_native {
                builder.intern(&name)
            } else {
                builder.intern(&lower_name)
            };
            let content = v
                .content
                .as_ref()
                .map(|c| builder.raw_span(c.start, c.end))
                .unwrap_or(builder.raw_span(v.tag_open.end, v.tag_open.end));
            let close = v
                .tag_close
                .as_ref()
                .map(|t| builder.raw_span(t.start, t.end));
            let close_name = v
                .tag_close
                .as_ref()
                .map(|t| builder.raw_span(t.start + 2, t.name_end));
            let full_end = close.map(|s| s.end).unwrap_or(content.end);
            let mut props: Vec<&verter_parser::types::NodeProp> = v.props.iter().collect();
            if let Some(c) = &v.v_condition {
                props.push(&c.prop)
            }
            if let Some(p) = &v.v_for {
                props.push(p)
            }
            if let Some(p) = &v.v_slot {
                props.push(p)
            }
            if let Some(p) = &v.v_once {
                props.push(p)
            }
            if let Some(p) = &v.v_ref {
                props.push(p)
            }
            props.sort_by_key(|p| p.start);
            let attributes = vue_attributes_refs(builder, &props);
            let child_parser_ids = v
                .content
                .as_ref()
                .map(|c| c.children.as_slice())
                .unwrap_or(&[]);
            let child_namespace =
                if namespace == MarkupNamespace::Svg && lower_name == "foreignobject" {
                    MarkupNamespace::Html
                } else {
                    namespace
                };
            let children = child_parser_ids
                .iter()
                .map(|child| {
                    project_vue_node(
                        builder,
                        ast,
                        *child,
                        root_block,
                        Some(placeholder),
                        child_namespace,
                    )
                })
                .collect();
            let void_element = namespace == MarkupNamespace::Html && is_void_html(&lower_name);
            (
                MarkupNodeKind::Element(MarkupElementSyntax {
                    authored_name: SourceSlice::new(name_span),
                    normalized_name: normalized,
                    namespace,
                    kind: if lower_name == "component" {
                        MarkupElementKind::DynamicComponent
                    } else if v.tag_type.is_component() && !parser_known_native {
                        MarkupElementKind::Component
                    } else {
                        MarkupElementKind::Native
                    },
                    opening_span: builder.raw_span(v.tag_open.start, v.tag_open.end),
                    opening_name_span: name_span,
                    attribute_insertion_anchor: builder.raw_span(
                        v.tag_open
                            .end
                            .saturating_sub(if v.is_self_closing { 2 } else { 1 }),
                        v.tag_open
                            .end
                            .saturating_sub(if v.is_self_closing { 2 } else { 1 }),
                    ),
                    content_span: content,
                    closing_span: close,
                    closing_name_span: close_name,
                    full_span: builder.raw_span(v.tag_open.start, full_end),
                    self_closing: v.is_self_closing,
                    void_element,
                    raw_text: matches!(
                        name.to_ascii_lowercase().as_str(),
                        "script" | "style" | "textarea"
                    ),
                    termination: if void_element {
                        SyntaxTermination::Void
                    } else if v.is_self_closing {
                        SyntaxTermination::SelfClosing
                    } else if close.is_some() {
                        SyntaxTermination::Closed
                    } else {
                        SyntaxTermination::UnclosedEof
                    },
                    attributes: Arc::from(attributes),
                }),
                children,
            )
        }
    };
    let start = builder.child_ids.len() as u32;
    builder.child_ids.extend(children);
    let end = builder.child_ids.len() as u32;
    builder.nodes[placeholder.0 as usize] = MarkupSyntaxNode {
        id: placeholder,
        root_block,
        parent,
        children: start..end,
        kind,
    };
    placeholder
}

fn is_void_html(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn vue_attributes(
    builder: &mut Builder<'_>,
    props: &[verter_parser::types::NodeProp],
) -> Vec<CarrierAttribute> {
    let refs = props.iter().collect::<Vec<_>>();
    vue_attributes_refs(builder, &refs)
}
fn vue_attributes_refs(
    builder: &mut Builder<'_>,
    props: &[&verter_parser::types::NodeProp],
) -> Vec<CarrierAttribute> {
    let mut duplicates: HashMap<String, AttributeId> = HashMap::new();
    props
        .iter()
        .map(|p| vue_attribute(builder, p, &mut duplicates))
        .collect()
}
fn vue_attribute(
    builder: &mut Builder<'_>,
    p: &verter_parser::types::NodeProp,
    duplicates: &mut HashMap<String, AttributeId>,
) -> CarrierAttribute {
    let id = builder.attribute_id();
    let name_text = builder.source[p.start as usize..p.name_end as usize].to_string();
    let full_end = attribute_full_end(builder.source, p);
    let full = builder.raw_span(p.start, full_end);
    if p.is_directive {
        let (known_family, prefix_len) = vue_directive(&name_text);
        let prefix = builder.raw_span(p.start, p.start + prefix_len as u32);
        let family = known_family.unwrap_or_else(|| {
            let normalized = builder.intern(&name_text[..prefix_len].to_ascii_lowercase());
            VueDirectiveKind::Custom {
                authored: SourceSlice::new(prefix),
                normalized,
            }
        });
        let argument = match (p.is_dynamic.unwrap_or(false), p.arg_start, p.arg_end) {
            (true, Some(start), Some(end)) => {
                // The tokenizer's EOF recovery still emits a dynamic `DirArg`
                // for an UNCLOSED `[` (X_MISSING_DYNAMIC_DIRECTIVE_ARGUMENT_END),
                // so the bracket geometry is fact-derived: a missing close
                // bracket projects a TYPED recovery argument (`close_span:
                // None`, `UnclosedEof`) — never a panic, never a fabricated
                // closed bracket.
                let authored = &builder.source[start as usize..end as usize];
                let open = authored.find('[');
                let close = open.and_then(|open| authored.rfind(']').filter(|close| *close > open));
                match (open, close) {
                    (Some(open), Some(close)) => {
                        let inner = &authored[open + 1..close];
                        let expression_start = inner.len() - inner.trim_start().len();
                        let expression_end = inner.trim_end().len();
                        let open = start + open as u32;
                        let close = start + close as u32;
                        DirectiveArgument::Dynamic {
                            full_span: builder.raw_span(open, close + 1),
                            open_span: builder.raw_span(open, open + 1),
                            expression_span: builder.raw_span(
                                open + 1 + expression_start as u32,
                                open + 1 + expression_end as u32,
                            ),
                            close_span: Some(builder.raw_span(close, close + 1)),
                            termination: SyntaxTermination::Closed,
                        }
                    }
                    (Some(open), None) => {
                        let inner = &authored[open + 1..];
                        let expression_start = inner.len() - inner.trim_start().len();
                        let expression_end = inner.trim_end().len();
                        let open = start + open as u32;
                        DirectiveArgument::Dynamic {
                            full_span: builder.raw_span(open, end),
                            open_span: builder.raw_span(open, open + 1),
                            expression_span: builder.raw_span(
                                open + 1 + expression_start as u32,
                                open + 1 + expression_end as u32,
                            ),
                            close_span: None,
                            termination: SyntaxTermination::UnclosedEof,
                        }
                    }
                    (None, _) => {
                        // A dynamic-flagged argument with no `[` at all has no
                        // live tokenizer producer; keep it recoverable-typed
                        // rather than panicking on a producer drift.
                        let expression_start = authored.len() - authored.trim_start().len();
                        let expression_end = authored.trim_end().len();
                        DirectiveArgument::Dynamic {
                            full_span: builder.raw_span(start, end),
                            open_span: builder.raw_span(start, start),
                            expression_span: builder.raw_span(
                                start + expression_start as u32,
                                start + expression_end as u32,
                            ),
                            close_span: None,
                            termination: SyntaxTermination::Recovered {
                                reason: verter_language::BlockRecoveryReason::ParserRejectedSyntax,
                                recovery_span: None,
                            },
                        }
                    }
                }
            }
            (false, Some(start), Some(end)) => {
                let authored = builder.raw_span(start, end);
                let normalized = builder.intern(&builder.source[start as usize..end as usize]);
                DirectiveArgument::Static {
                    name: AttributeName {
                        authored: SourceSlice::new(authored),
                        normalized,
                        name_span: authored,
                    },
                }
            }
            _ => DirectiveArgument::None,
        };
        let modifiers = p
            .modifiers
            .iter()
            .map(|m| {
                let text = builder.source[m.start as usize..m.end as usize].to_string();
                let normalized = builder.intern(&text.to_ascii_lowercase());
                DirectiveModifier {
                    authored: SourceSlice::new(builder.span(*m)),
                    normalized,
                    separator_span: builder.raw_span(m.start.saturating_sub(1), m.start),
                    name_span: builder.span(*m),
                    full_span: builder.raw_span(m.start.saturating_sub(1), m.end),
                }
            })
            .collect::<Vec<_>>();
        let value = vue_value(builder, p, true);
        let key = format!("d:{name_text}");
        let duplicate_of = duplicates.insert(key, id);
        CarrierAttribute::Directive {
            id,
            family: DirectiveFamily::Vue(family),
            prefix_span: prefix,
            local_name: None,
            argument,
            modifiers: Arc::from(modifiers),
            value,
            full_span: full,
            duplicate_of,
        }
    } else {
        let name_span = builder.raw_span(p.start, p.name_end);
        let normalized_text = name_text.to_ascii_lowercase();
        let normalized = builder.intern(&normalized_text);
        let duplicate_of = duplicates.insert(normalized_text, id);
        CarrierAttribute::Named {
            id,
            name: AttributeName {
                authored: SourceSlice::new(name_span),
                normalized,
                name_span,
            },
            syntax: NamedAttributeSyntax::Explicit,
            value: vue_value(builder, p, false),
            full_span: full,
            duplicate_of,
        }
    }
}
/// Classify a Vue directive family. `None` marks a userland (custom) family —
/// the caller mints the payload-carrying [`VueDirectiveKind::Custom`] with the
/// authored family slice and its normalized name.
fn vue_directive(name: &str) -> (Option<VueDirectiveKind>, usize) {
    if name.starts_with(':') {
        return (Some(VueDirectiveKind::Bind), 1);
    }
    if name.starts_with('@') {
        return (Some(VueDirectiveKind::On), 1);
    }
    if name.starts_with('#') {
        return (Some(VueDirectiveKind::Slot), 1);
    }
    let family = name.split([':', '.']).next().unwrap_or(name);
    let kind = match family {
        "v-bind" => Some(VueDirectiveKind::Bind),
        "v-on" => Some(VueDirectiveKind::On),
        "v-model" => Some(VueDirectiveKind::Model),
        "v-show" => Some(VueDirectiveKind::Show),
        "v-if" => Some(VueDirectiveKind::If),
        "v-else-if" => Some(VueDirectiveKind::ElseIf),
        "v-else" => Some(VueDirectiveKind::Else),
        "v-for" => Some(VueDirectiveKind::For),
        "v-slot" => Some(VueDirectiveKind::Slot),
        "v-pre" => Some(VueDirectiveKind::Pre),
        "v-cloak" => Some(VueDirectiveKind::Cloak),
        "v-once" => Some(VueDirectiveKind::Once),
        "v-memo" => Some(VueDirectiveKind::Memo),
        "v-html" => Some(VueDirectiveKind::Html),
        "v-text" => Some(VueDirectiveKind::Text),
        _ => None,
    };
    (kind, family.len())
}
fn vue_value(
    builder: &mut Builder<'_>,
    p: &verter_parser::types::NodeProp,
    dynamic: bool,
) -> AttributeValue {
    match p.value_start.zip(p.value_end) {
        None => AttributeValue::Missing,
        Some((s, e)) if dynamic => AttributeValue::Expression {
            syntax: AttributeDynamicSyntax::VueBracedExpression,
            full_span: match quote_at(builder.source, s) {
                AttributeQuote::Unquoted => builder.raw_span(s, e),
                _ => builder.raw_span(s - 1, e + 1),
            },
            open_span: match quote_at(builder.source, s) {
                AttributeQuote::Unquoted => None,
                _ => Some(builder.raw_span(s - 1, s)),
            },
            expression_span: builder.raw_span(s, e),
            close_span: match quote_at(builder.source, s) {
                AttributeQuote::Unquoted => None,
                _ => Some(builder.raw_span(e, e + 1)),
            },
            termination: SyntaxTermination::Closed,
        },
        Some((s, e)) => {
            let quote = quote_at(builder.source, s);
            let raw = SourceSlice::new(builder.raw_span(s, e));
            let decoded = if builder.source[s as usize..e as usize].contains('&') {
                LazyDecodedText::EntityDecode {
                    key: DecodedValueKey {
                        raw,
                        recipe: EntityDecodeRecipe::Html5Attribute { quote },
                    },
                }
            } else {
                LazyDecodedText::SameAsSource
            };
            let value_span = match quote {
                AttributeQuote::Unquoted => builder.raw_span(s, e),
                _ => builder.raw_span(s - 1, e + 1),
            };
            AttributeValue::Static {
                raw,
                decoded,
                quote,
                value_span,
                inner_span: builder.raw_span(s, e),
            }
        }
    }
}
fn quote_at(source: &str, start: u32) -> AttributeQuote {
    match source.as_bytes().get(start.saturating_sub(1) as usize) {
        Some(b'\'') => AttributeQuote::Single,
        Some(b'\"') => AttributeQuote::Double,
        _ => AttributeQuote::Unquoted,
    }
}
fn attribute_full_end(source: &str, p: &verter_parser::types::NodeProp) -> u32 {
    p.value_end
        .map(|e| match quote_at(source, p.value_start.unwrap_or(e)) {
            AttributeQuote::Unquoted => e,
            _ => e + 1,
        })
        .unwrap_or(p.name_end)
}

fn project_svelte(
    svelte: &SvelteCarrierCompiler,
    accepted: &AcceptedRegisteredCarrierSource,
    artifact: &UnregisteredFrameworkParseArtifact,
) -> Result<CarrierBlockInventory, InventoryValidationError> {
    let parsed = svelte
        .unregistered_parsed_svelte(artifact)
        .expect("Svelte artifact");
    enum Root<'a> {
        Script(&'a crate::svelte::parser::SvelteScript),
        Style(&'a crate::svelte::parser::SvelteStyle),
        Markup(&'a crate::svelte::parser::SvelteNode),
    }
    impl Root<'_> {
        fn start(&self) -> u32 {
            match self {
                Self::Script(v) => v.tag_open.start,
                Self::Style(v) => v.tag_open.start,
                Self::Markup(v) => svelte_node_span(v).start,
            }
        }
    }
    let mut roots = Vec::new();
    roots.extend(parsed.instance_script.iter().map(Root::Script));
    roots.extend(parsed.module_script.iter().map(Root::Script));
    roots.extend(parsed.styles.iter().map(Root::Style));
    roots.extend(parsed.template.iter().map(Root::Markup));
    roots.sort_by_key(Root::start);
    let mut builder = Builder::new(accepted.source().bytes());
    let mut blocks = Vec::new();
    for root in roots {
        let id = BlockId(blocks.len() as u32);
        match root {
            Root::Script(v) => {
                let syntax = svelte_tagged(
                    &mut builder,
                    "script",
                    v.tag_open,
                    v.content,
                    v.tag_close,
                    &v.attributes,
                );
                let dialect = ScriptSourceType::from(
                    crate::svelte::carrier::svelte_script_source_type(Some(v)),
                );
                blocks.push(CarrierBlock::Section {
                    id,
                    role: SectionRole::Script {
                        role: if v.is_module {
                            ScriptRole::Module
                        } else {
                            ScriptRole::Instance
                        },
                        dialect,
                    },
                    syntax,
                });
            }
            Root::Style(v) => {
                let syntax = svelte_tagged(
                    &mut builder,
                    "style",
                    v.tag_open,
                    v.content,
                    v.tag_close,
                    &v.attributes,
                );
                // Dialect derives from the parser-owned `lang` (mirrors the Vue
                // path: recognised names map, an unrecognised name is Missing,
                // no lang is CSS). Svelte has no authored `scoped` / `module`
                // attributes, so those stay un-fabricated.
                let dialect = match v.lang.as_deref() {
                    None => StyleDialect::Css,
                    Some(lang) => match lang.to_ascii_lowercase().as_str() {
                        "css" => StyleDialect::Css,
                        "scss" => StyleDialect::Scss,
                        "sass" => StyleDialect::Sass,
                        "less" => StyleDialect::Less,
                        "stylus" => StyleDialect::Stylus,
                        "postcss" => StyleDialect::PostCss,
                        _ => StyleDialect::Missing,
                    },
                };
                blocks.push(CarrierBlock::Section {
                    id,
                    role: SectionRole::Style {
                        dialect,
                        scoped: false,
                        module: StyleModule::None,
                    },
                    syntax,
                });
            }
            Root::Markup(v) => {
                let node = project_svelte_node(&mut builder, v, id, None, MarkupNamespace::Html);
                builder.roots.push(node);
                blocks.push(CarrierBlock::MarkupRoot { id, node });
            }
        }
    }
    builder.finish(accepted, blocks)
}

fn svelte_tagged(
    builder: &mut Builder<'_>,
    name: &str,
    open: Span,
    content: Option<Span>,
    close: Option<Span>,
    attrs: &[crate::svelte::parser::SvelteAttribute],
) -> TaggedSyntax {
    let name_start = open.start + 1;
    let name_span = builder.raw_span(name_start, name_start + name.len() as u32);
    let normalized = builder.intern(name);
    let content = content
        .map(|s| builder.span(s))
        .unwrap_or(builder.raw_span(open.end, open.end));
    let attributes = svelte_attributes(builder, attrs);
    TaggedSyntax {
        authored_name: SourceSlice::new(name_span),
        normalized_name: normalized,
        opening_span: builder.span(open),
        opening_name_span: name_span,
        attribute_insertion_anchor: builder
            .raw_span(open.end.saturating_sub(1), open.end.saturating_sub(1)),
        content_span: content,
        closing_span: close.map(|s| builder.span(s)),
        closing_name_span: close
            .map(|s| builder.raw_span(s.start + 2, s.start + 2 + name.len() as u32)),
        full_span: builder.raw_span(open.start, close.map(|s| s.end).unwrap_or(content.end)),
        termination: if close.is_some() {
            SyntaxTermination::Closed
        } else {
            SyntaxTermination::UnclosedEof
        },
        attributes: Arc::from(attributes),
    }
}

fn project_svelte_node(
    builder: &mut Builder<'_>,
    node: &crate::svelte::parser::SvelteNode,
    root_block: BlockId,
    parent: Option<MarkupNodeId>,
    parent_namespace: MarkupNamespace,
) -> MarkupNodeId {
    use crate::svelte::parser::{
        SvelteBlockKind, SvelteClauseKind, SvelteElementKind, SvelteNode, SvelteSpecialKind,
        SvelteTagKind,
    };
    let placeholder = MarkupNodeId(builder.nodes.len() as u32);
    builder.nodes.push(MarkupSyntaxNode {
        id: placeholder,
        root_block,
        parent,
        children: 0..0,
        kind: MarkupNodeKind::Text {
            content_span: builder.raw_span(0, 0),
        },
    });
    let (kind, children) = match node {
        SvelteNode::Text(span) => (
            MarkupNodeKind::Text {
                content_span: builder.span(*span),
            },
            vec![],
        ),
        SvelteNode::Comment(span) => {
            let closed = span.end >= span.start + 7;
            (
                MarkupNodeKind::Comment {
                    opening_span: builder.raw_span(span.start, (span.start + 4).min(span.end)),
                    content_span: builder.raw_span(
                        (span.start + 4).min(span.end),
                        span.end.saturating_sub(if closed { 3 } else { 0 }),
                    ),
                    closing_span: closed.then(|| builder.raw_span(span.end - 3, span.end)),
                    full_span: builder.span(*span),
                    termination: if closed {
                        SyntaxTermination::Closed
                    } else {
                        SyntaxTermination::UnclosedEof
                    },
                },
                vec![],
            )
        }
        SvelteNode::Interpolation(span) => (
            MarkupNodeKind::Interpolation {
                family: MarkupInterpolationFamily::SvelteInterpolation,
                opening_span: builder.raw_span(span.start.saturating_sub(1), span.start),
                expression_span: builder.span(*span),
                closing_span: Some(builder.raw_span(span.end, span.end + 1)),
                full_span: builder.raw_span(span.start.saturating_sub(1), span.end + 1),
                termination: SyntaxTermination::Closed,
            },
            vec![],
        ),
        SvelteNode::Element(v) => {
            let normalized_text = match v.kind {
                SvelteElementKind::Intrinsic | SvelteElementKind::NestedStyle => {
                    v.name.to_ascii_lowercase()
                }
                _ => v.name.clone(),
            };
            let normalized = builder.intern(&normalized_text);
            let lower_name = v.name.to_ascii_lowercase();
            let namespace = match lower_name.as_str() {
                "svg" => MarkupNamespace::Svg,
                "math" => MarkupNamespace::MathMl,
                _ => parent_namespace,
            };
            let attributes = svelte_attributes(builder, &v.attributes);
            let child_namespace =
                if namespace == MarkupNamespace::Svg && lower_name == "foreignobject" {
                    MarkupNamespace::Html
                } else {
                    namespace
                };
            let child_ids = v
                .children
                .iter()
                .map(|child| {
                    project_svelte_node(
                        builder,
                        child,
                        root_block,
                        Some(placeholder),
                        child_namespace,
                    )
                })
                .collect();
            let content = if let Some(first) = v.children.first() {
                let first = svelte_node_span(first).start;
                let end = v.close_span.map(|s| s.start).unwrap_or_else(|| {
                    v.children
                        .last()
                        .map(svelte_node_span)
                        .map(|s| s.end)
                        .unwrap_or(v.open_span.end)
                });
                builder.raw_span(first, end)
            } else {
                builder.raw_span(
                    v.open_span.end,
                    v.close_span.map(|s| s.start).unwrap_or(v.open_span.end),
                )
            };
            let kind = match v.kind {
                SvelteElementKind::Intrinsic => MarkupElementKind::Native,
                SvelteElementKind::Component => MarkupElementKind::Component,
                SvelteElementKind::NestedStyle => MarkupElementKind::SvelteNestedStyle,
                SvelteElementKind::Special(value) => {
                    MarkupElementKind::SvelteSpecial(match value {
                        SvelteSpecialKind::Head => SvelteSpecialElementKind::Head,
                        SvelteSpecialKind::Window => SvelteSpecialElementKind::Window,
                        SvelteSpecialKind::Document => SvelteSpecialElementKind::Document,
                        SvelteSpecialKind::Body => SvelteSpecialElementKind::Body,
                        SvelteSpecialKind::Element => SvelteSpecialElementKind::Element,
                        SvelteSpecialKind::Boundary => SvelteSpecialElementKind::Boundary,
                        SvelteSpecialKind::Options => SvelteSpecialElementKind::Options,
                        SvelteSpecialKind::Component => SvelteSpecialElementKind::Component,
                        SvelteSpecialKind::SelfRef => SvelteSpecialElementKind::SelfRef,
                        SvelteSpecialKind::Fragment => SvelteSpecialElementKind::Fragment,
                        SvelteSpecialKind::Unknown => SvelteSpecialElementKind::Unknown {
                            authored_local: SourceSlice::new(builder.raw_span(
                                v.name_span.start + "svelte:".len() as u32,
                                v.name_span.end,
                            )),
                        },
                    })
                }
            };
            let full_end = v.close_span.map(|s| s.end).unwrap_or_else(|| {
                if v.self_closing {
                    v.open_span.end
                } else {
                    content.end
                }
            });
            let void_element = namespace == MarkupNamespace::Html && is_void_html(&lower_name);
            (
                MarkupNodeKind::Element(MarkupElementSyntax {
                    authored_name: builder.slice(v.name_span),
                    normalized_name: normalized,
                    namespace,
                    kind,
                    opening_span: builder.span(v.open_span),
                    opening_name_span: builder.span(v.name_span),
                    attribute_insertion_anchor: builder.raw_span(
                        v.open_span
                            .end
                            .saturating_sub(if v.self_closing { 2 } else { 1 }),
                        v.open_span
                            .end
                            .saturating_sub(if v.self_closing { 2 } else { 1 }),
                    ),
                    content_span: content,
                    closing_span: v.close_span.map(|s| builder.span(s)),
                    closing_name_span: v
                        .close_span
                        .map(|s| builder.raw_span(s.start + 2, s.start + 2 + v.name.len() as u32)),
                    full_span: builder.raw_span(v.open_span.start, full_end),
                    self_closing: v.self_closing,
                    void_element,
                    raw_text: matches!(v.kind, SvelteElementKind::NestedStyle),
                    termination: if void_element {
                        SyntaxTermination::Void
                    } else if v.self_closing {
                        SyntaxTermination::SelfClosing
                    } else if v.close_span.is_some() {
                        SyntaxTermination::Closed
                    } else {
                        SyntaxTermination::UnclosedEof
                    },
                    attributes: Arc::from(attributes),
                }),
                child_ids,
            )
        }
        SvelteNode::Block(v) => {
            let mut child_ids = v
                .children
                .iter()
                .map(|child| {
                    project_svelte_node(
                        builder,
                        child,
                        root_block,
                        Some(placeholder),
                        parent_namespace,
                    )
                })
                .collect::<Vec<_>>();
            for clause in &v.clauses {
                let clause_children = clause
                    .children
                    .iter()
                    .map(|child| {
                        project_svelte_node(builder, child, root_block, None, parent_namespace)
                    })
                    .collect::<Vec<_>>();
                let head = match clause.kind {
                    SvelteClauseKind::Else => SvelteClauseHead::Else,
                    SvelteClauseKind::ElseIf => SvelteClauseHead::ElseIf {
                        // Parser-owned fact: an authored empty condition
                        // (`{:else if}`) is `None` — typed recovery, never a
                        // panic or a fabricated span.
                        condition: clause.expr.map(|s| builder.span(s)),
                    },
                    SvelteClauseKind::Then => SvelteClauseHead::Then {
                        binding: clause.expr.map(|s| builder.span(s)),
                    },
                    SvelteClauseKind::Catch => SvelteClauseHead::Catch {
                        binding: clause.expr.map(|s| builder.span(s)),
                    },
                };
                let clause_id = builder.add_node(
                    root_block,
                    Some(placeholder),
                    MarkupNodeKind::SvelteClause(SvelteClauseSyntax {
                        head,
                        marker_span: builder.span(clause.tag_span),
                        full_span: builder.span(clause.tag_span),
                        termination: SyntaxTermination::Closed,
                    }),
                    clause_children,
                );
                for child in builder.nodes.iter_mut().filter(|n| {
                    n.parent.is_none() && n.root_block == root_block && n.id != placeholder
                }) {
                    child.parent = Some(clause_id);
                }
                child_ids.push(clause_id);
            }
            if let SvelteBlockKind::Unknown { keyword } = &v.kind {
                // An unrecognised `{#keyword}` block projects as an UNKNOWN
                // node (parser-owned classification): no known block family is
                // fabricated and no head expression is demanded.
                let content_end = v.close_tag.map(|s| s.start).unwrap_or(v.span.end);
                let kind = MarkupNodeKind::Unknown {
                    opening_span: Some(builder.span(v.head_span)),
                    opening_name_span: Some(builder.span(*keyword)),
                    content_span: Some(builder.raw_span(v.head_span.end, content_end)),
                    closing_span: v.close_tag.map(|s| builder.span(s)),
                    closing_name_span: None,
                    full_span: builder.span(v.span),
                    termination: if v.close_tag.is_some() {
                        SyntaxTermination::Closed
                    } else {
                        SyntaxTermination::Recovered {
                            reason: verter_language::BlockRecoveryReason::MissingCloseTag,
                            recovery_span: None,
                        }
                    },
                    authored_head: Some(SourceSlice::new(builder.span(*keyword))),
                    reason: UnknownMarkupReason::ParserUnknownVariant,
                };
                let start = builder.child_ids.len() as u32;
                builder.child_ids.extend(child_ids);
                let end = builder.child_ids.len() as u32;
                builder.nodes[placeholder.0 as usize] = MarkupSyntaxNode {
                    id: placeholder,
                    root_block,
                    parent,
                    children: start..end,
                    kind,
                };
                return placeholder;
            }
            // Parser-owned head facts: an EMPTY authored head (`{#if}`,
            // `{#each}`, `{#await}`, `{#key}` — ordinary mid-typing states)
            // yields `head_expr: None` from the tokenizer and projects the
            // typed `None` recovery — never a panic, never a fabricated span.
            let head = match &v.kind {
                SvelteBlockKind::If => SvelteControlBlockHead::If {
                    condition: v.head_expr.map(|s| builder.span(s)),
                },
                SvelteBlockKind::Each { item, index, key } => SvelteControlBlockHead::Each {
                    iterable: v.head_expr.map(|s| builder.span(s)),
                    item: item.map(|s| builder.span(s)),
                    index: index.map(|s| builder.span(s)),
                    key: key.map(|s| builder.span(s)),
                },
                SvelteBlockKind::Await { inline_branch, .. } => SvelteControlBlockHead::Await {
                    promise: v.head_expr.map(|s| builder.span(s)),
                    inline_branch: match inline_branch {
                        crate::svelte::parser::SvelteAwaitInline::None => {
                            SvelteAwaitInlineBranch::None
                        }
                        crate::svelte::parser::SvelteAwaitInline::Then {
                            marker_span,
                            head_span,
                            binding,
                        } => SvelteAwaitInlineBranch::Then {
                            marker_span: builder.span(*marker_span),
                            head_span: builder.span(*head_span),
                            binding: binding.map(|span| builder.span(span)),
                        },
                        crate::svelte::parser::SvelteAwaitInline::Catch {
                            marker_span,
                            head_span,
                            binding,
                        } => SvelteAwaitInlineBranch::Catch {
                            marker_span: builder.span(*marker_span),
                            head_span: builder.span(*head_span),
                            binding: binding.map(|span| builder.span(span)),
                        },
                    },
                },
                SvelteBlockKind::Key => SvelteControlBlockHead::Key {
                    expression: v.head_expr.map(|s| builder.span(s)),
                },
                SvelteBlockKind::Snippet {
                    name,
                    name_text: _,
                    params,
                } => SvelteControlBlockHead::Snippet {
                    authored_name: SourceSlice::new(builder.span(*name)),
                    name_span: builder.span(*name),
                    params_span: params.map(|s| builder.span(s)),
                },
                SvelteBlockKind::Unknown { .. } => {
                    unreachable!("unknown blocks project as MarkupNodeKind::Unknown above")
                }
            };
            (
                MarkupNodeKind::SvelteControlBlock(SvelteControlBlockSyntax {
                    head,
                    opening_span: builder.span(v.head_span),
                    // Parser-owned close geometry: the consumed `{/keyword}`
                    // span when present; a missing close marker projects
                    // typed recovery — never a fabricated Closed.
                    closing_span: v.close_tag.map(|s| builder.span(s)),
                    full_span: builder.span(v.span),
                    termination: if v.close_tag.is_some() {
                        SyntaxTermination::Closed
                    } else {
                        SyntaxTermination::Recovered {
                            reason: verter_language::BlockRecoveryReason::MissingCloseTag,
                            recovery_span: None,
                        }
                    },
                }),
                child_ids,
            )
        }
        SvelteNode::Tag(v) => {
            let family = match v.kind {
                SvelteTagKind::Render => SvelteStandaloneTagFamily::Render,
                SvelteTagKind::Html => SvelteStandaloneTagFamily::Html,
                SvelteTagKind::LegacyConst => SvelteStandaloneTagFamily::LegacyConst,
                SvelteTagKind::Const => SvelteStandaloneTagFamily::Const,
                SvelteTagKind::Let => SvelteStandaloneTagFamily::Let,
                SvelteTagKind::Debug => SvelteStandaloneTagFamily::Debug,
                SvelteTagKind::Attach => SvelteStandaloneTagFamily::Attach,
                SvelteTagKind::Unknown => SvelteStandaloneTagFamily::Unknown {
                    // Parser-owned keyword span (`@wat`) — the authored NAME,
                    // never the tag's expression payload.
                    authored_name: SourceSlice::new(builder.span(v.keyword)),
                    reason: UnknownMarkupReason::ParserUnknownVariant,
                },
            };
            (
                MarkupNodeKind::SvelteStandaloneTag(SvelteStandaloneTagSyntax {
                    family,
                    opening_span: builder.raw_span(v.span.start, v.inner.start),
                    expression_span: Some(builder.span(v.inner)),
                    closing_span: Some(builder.raw_span(v.inner.end, v.span.end)),
                    full_span: builder.span(v.span),
                    termination: SyntaxTermination::Closed,
                }),
                vec![],
            )
        }
    };
    let start = builder.child_ids.len() as u32;
    builder.child_ids.extend(children);
    let end = builder.child_ids.len() as u32;
    builder.nodes[placeholder.0 as usize] = MarkupSyntaxNode {
        id: placeholder,
        root_block,
        parent,
        children: start..end,
        kind,
    };
    placeholder
}

fn svelte_attributes(
    builder: &mut Builder<'_>,
    attrs: &[crate::svelte::parser::SvelteAttribute],
) -> Vec<CarrierAttribute> {
    use crate::svelte::parser::{SvelteAttributeKind, SvelteDirectiveKind as K};
    let mut duplicates = HashMap::new();
    attrs
        .iter()
        .map(|attr| {
            let id = builder.attribute_id();
            match &attr.kind {
                SvelteAttributeKind::Plain {
                    name,
                    name_span,
                    value,
                } => {
                    let shorthand = !name.is_empty()
                        && matches!(value, Some(crate::svelte::parser::SvelteAttributeValue::Expression(expression)) if attr.span.start + 1 == name_span.start && expression.end + 1 == attr.span.end);
                    let normalized_text = name.to_ascii_lowercase();
                    let normalized = builder.intern(&normalized_text);
                    let duplicate_of = duplicates.insert(normalized_text, id);
                    CarrierAttribute::Named {
                        id,
                        name: AttributeName {
                            authored: builder.slice(*name_span),
                            normalized,
                            name_span: builder.span(*name_span),
                        },
                        syntax: if shorthand {
                            NamedAttributeSyntax::SvelteShorthand
                        } else {
                            NamedAttributeSyntax::Explicit
                        },
                        value: svelte_value(
                            builder,
                            value.as_ref(),
                            &attr.mixed_parts,
                            shorthand,
                        ),
                        full_span: builder.span(attr.span),
                        duplicate_of,
                    }
                }
                SvelteAttributeKind::Spread(expr) => CarrierAttribute::Spread {
                    id,
                    full_span: builder.span(attr.span),
                    open_span: builder.raw_span(attr.span.start, expr.start),
                    expression_span: builder.span(*expr),
                    close_span: (expr.end < attr.span.end)
                        .then(|| builder.raw_span(expr.end, attr.span.end)),
                    termination: SyntaxTermination::Closed,
                },
                SvelteAttributeKind::Directive(v) => {
                    let prefix_text = v.prefix.as_str();
                    let family = match v.kind {
                        K::Bind => SvelteDirectiveKind::Bind,
                        K::Class => SvelteDirectiveKind::Class,
                        K::Style => SvelteDirectiveKind::Style,
                        K::Use => SvelteDirectiveKind::Use,
                        K::Transition => SvelteDirectiveKind::Transition,
                        K::In => SvelteDirectiveKind::In,
                        K::Out => SvelteDirectiveKind::Out,
                        K::Animate => SvelteDirectiveKind::Animate,
                        K::On => SvelteDirectiveKind::On,
                        K::Let => SvelteDirectiveKind::Let,
                        K::Unknown => SvelteDirectiveKind::Unknown {
                            authored_family: SourceSlice::new(builder.raw_span(
                                attr.span.start,
                                attr.span.start + prefix_text.len() as u32,
                            )),
                            reason: UnknownDirectiveReason::ParserUnknownVariant,
                        },
                    };
                    let local_start = attr.span.start + prefix_text.len() as u32 + 1;
                    let local_span =
                        builder.raw_span(local_start, local_start + v.local.len() as u32);
                    let local_normalized = builder.intern(&v.local);
                    let modifiers = v
                        .modifiers
                        .iter()
                        .zip(&v.modifier_spans)
                        .map(|(m, span)| {
                            let normalized = builder.intern(m);
                            DirectiveModifier {
                                authored: builder.slice(*span),
                                normalized,
                                separator_span: builder
                                    .raw_span(span.start.saturating_sub(1), span.start),
                                name_span: builder.span(*span),
                                full_span: builder
                                    .raw_span(span.start.saturating_sub(1), span.end),
                            }
                        })
                        .collect::<Vec<_>>();
                    CarrierAttribute::Directive {
                        id,
                        family: DirectiveFamily::Svelte(family),
                        prefix_span: builder
                            .raw_span(attr.span.start, attr.span.start + prefix_text.len() as u32),
                        local_name: Some(AttributeName {
                            authored: SourceSlice::new(local_span),
                            normalized: local_normalized,
                            name_span: local_span,
                        }),
                        argument: DirectiveArgument::None,
                        modifiers: Arc::from(modifiers),
                        value: v
                            .value
                            .as_ref()
                            .map(|v| svelte_value(builder, Some(v), &attr.mixed_parts, false))
                            .unwrap_or(AttributeValue::Missing),
                        full_span: builder.span(attr.span),
                        duplicate_of: None,
                    }
                }
                SvelteAttributeKind::Attach { expr_span } => CarrierAttribute::Attach {
                    id,
                    full_span: builder.span(attr.span),
                    keyword_span: builder
                        .raw_span(attr.span.start, (attr.span.start + 8).min(attr.span.end)),
                    expression_span: builder.span(*expr_span),
                    close_span: (expr_span.end < attr.span.end)
                        .then(|| builder.raw_span(expr_span.end, attr.span.end)),
                    termination: SyntaxTermination::Closed,
                },
            }
        })
        .collect()
}
fn svelte_value(
    builder: &mut Builder<'_>,
    value: Option<&crate::svelte::parser::SvelteAttributeValue>,
    mixed_parts: &[crate::svelte::parser::SvelteMixedAttributePart],
    shorthand: bool,
) -> AttributeValue {
    use crate::svelte::parser::SvelteAttributeValue;
    match value {
        None => AttributeValue::Missing,
        Some(SvelteAttributeValue::Text(span)) => {
            let quote = quote_at(builder.source, span.start);
            let raw = builder.slice(*span);
            AttributeValue::Static {
                raw,
                decoded: if builder.source[span.start as usize..span.end as usize].contains('&') {
                    LazyDecodedText::EntityDecode {
                        key: DecodedValueKey {
                            raw,
                            recipe: EntityDecodeRecipe::SvelteAttribute { quote },
                        },
                    }
                } else {
                    LazyDecodedText::SameAsSource
                },
                quote,
                value_span: match quote {
                    AttributeQuote::Unquoted => builder.span(*span),
                    _ => builder.raw_span(span.start - 1, span.end + 1),
                },
                inner_span: builder.span(*span),
            }
        }
        Some(SvelteAttributeValue::Expression(span)) => AttributeValue::Expression {
            syntax: if shorthand {
                AttributeDynamicSyntax::SvelteShorthand
            } else {
                AttributeDynamicSyntax::SvelteMustacheExpression
            },
            full_span: builder.raw_span(span.start.saturating_sub(1), span.end + 1),
            open_span: Some(builder.raw_span(span.start.saturating_sub(1), span.start)),
            expression_span: builder.span(*span),
            close_span: Some(builder.raw_span(span.end, span.end + 1)),
            termination: SyntaxTermination::Closed,
        },
        Some(SvelteAttributeValue::Mixed(span)) => {
            let quote = quote_at(builder.source, span.start);
            let parts = mixed_parts
                .iter()
                .map(|part| match part {
                    crate::svelte::parser::SvelteMixedAttributePart::Text(part_span) => {
                        let raw = builder.slice(*part_span);
                        let decoded = if builder.source
                            [part_span.start as usize..part_span.end as usize]
                            .contains('&')
                        {
                            LazyDecodedText::EntityDecode {
                                key: DecodedValueKey {
                                    raw,
                                    recipe: EntityDecodeRecipe::SvelteAttribute { quote },
                                },
                            }
                        } else {
                            LazyDecodedText::SameAsSource
                        };
                        AttributeValuePart::Static { raw, decoded }
                    }
                    crate::svelte::parser::SvelteMixedAttributePart::Expression(expression) => {
                        AttributeValuePart::Expression {
                            syntax: AttributeDynamicSyntax::SvelteMustacheExpression,
                            full_span: builder
                                .raw_span(expression.start.saturating_sub(1), expression.end + 1),
                            open_span: Some(
                                builder
                                    .raw_span(expression.start.saturating_sub(1), expression.start),
                            ),
                            expression_span: builder.span(*expression),
                            close_span: Some(builder.raw_span(expression.end, expression.end + 1)),
                            termination: SyntaxTermination::Closed,
                        }
                    }
                })
                .collect::<Vec<_>>();
            AttributeValue::Mixed {
                full_span: match quote {
                    AttributeQuote::Unquoted => builder.span(*span),
                    _ => builder.raw_span(span.start - 1, span.end + 1),
                },
                parts: Arc::from(parts),
            }
        }
    }
}
fn svelte_node_span(node: &crate::svelte::parser::SvelteNode) -> Span {
    use crate::svelte::parser::SvelteNode;
    match node {
        SvelteNode::Text(s) | SvelteNode::Comment(s) => *s,
        SvelteNode::Interpolation(s) => Span::new(s.start.saturating_sub(1), s.end + 1),
        SvelteNode::Element(v) => Span::new(
            v.open_span.start,
            v.close_span.map(|s| s.end).unwrap_or(v.open_span.end),
        ),
        SvelteNode::Block(v) => v.span,
        SvelteNode::Tag(v) => v.span,
    }
}
