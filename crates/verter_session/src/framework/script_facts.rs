#![deny(missing_docs)]
//! The resolved-validation half of the framework script-fact seam.
//!
//! `verter_semantic` owns the syntax-capture half (the [`ScriptFactProvider`]
//! trait, its syntax-only `capture`, the per-file parse-domain candidate
//! collection). This module owns the session-side resolved-validation half: it
//! takes a file's captured candidates and validates them against resolved
//! import sources + derived capability bits, producing the typed resolved facts
//! an adapter retrieves through
//! [`FrameworkAdapterCtx::script_facts_for`](crate::framework::ctx::FrameworkAdapterCtx::script_facts_for).
//!
//! Two stores back the half, both keyed exactly as the cache architecture
//! requires:
//! - [`FrameworkScriptCandidateStore`] — a CONTENT-ADDRESSED artifact store. Its
//!   key carries the file's content/version dimensions
//!   (`canonical`, `content_hash`, `parse_env_hash`, `parser_version`,
//!   `file_language_id`) plus `(provider_id, provider_version)`. A content edit
//!   or a provider upgrade misses the stale slot (the content-addressed
//!   family).
//! - [`FrameworkScriptFactStore`] — the RESOLVED-fact store. Its sub-key is
//!   `(canonical, provider_id, provider_version, consumed_capability_bits,
//!   project_identity, resolve_env_hash)` — NO `lib_env_hash` / `type_env_hash`
//!   (the resolved facts do not depend on lib/type data). Warm reads pass TWO
//!   gates: a strict same-generation gate AND a `ReadSetSignature.facts`
//!   validation against the caller's live view; publication is ONLY via
//!   [`SignatureAdmission::Cacheable`] — an overflowed signature returns the
//!   computed value to the caller alone and never warms the store.
//!
//! No production provider registers in this program — Vue's macro analysis
//! stays inside the shallow pass — so [`resolve_script_facts`] is a zero-cost
//! `None` whenever the registry's [`ActiveProviderIndex`] is empty. The seam is
//! exercised end-to-end by an in-tree fixture provider.

use std::sync::Arc;

use dashmap::DashMap;
use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;
use verter_language::FrameworkAdapterId;
use verter_semantic::analysis::framework_facts::{
    capture_script_candidates, FrameworkScriptCandidates, FrameworkScriptFactPayload,
    ResolvedImportTarget, ResolvedValidationCx, ScriptFactProvider,
};

use crate::cache_runtime::SignatureAdmission;
use crate::fact_signature_helpers::ReadSetSignature;
use crate::framework::registry::FrameworkRegistration;
use crate::resolver_core::{ResolverContext, StoreView};
use crate::types::Hash16;
use crate::VerterHost;

/// The content-addressed candidate-store key.
///
/// Carries the full file content/version identity plus the capturing provider.
/// A content edit changes `content_hash`; a provider upgrade changes
/// `provider_version`; either misses the stale slot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CandidateSlotKey {
    /// The owner file canonical.
    pub canonical: Arc<str>,
    /// The file's content identity.
    pub content_hash: Hash16,
    /// The parse-env dimension.
    pub parse_env_hash: Hash16,
    /// The parser version.
    pub parser_version: u32,
    /// The file's `FileLanguage` row.
    pub file_language_id: verter_language::FileLanguage,
    /// The capturing provider's adapter id.
    pub provider_id: FrameworkAdapterId,
    /// The capturing provider's version.
    pub provider_version: u32,
}

/// The content-addressed framework-script CANDIDATE store.
///
/// A pure content-addressed artifact cache: a content edit or provider upgrade
/// changes the key and forces a cold re-capture. Hands out immutable `Arc`s.
#[derive(Debug, Default)]
pub struct FrameworkScriptCandidateStore {
    entries: DashMap<CandidateSlotKey, Arc<FrameworkScriptCandidates>>,
}

impl FrameworkScriptCandidateStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The cached candidates for `key`, if present. Content-addressed: a hit is
    /// always valid (the key carries every content/version dimension).
    #[must_use]
    pub fn get(&self, key: &CandidateSlotKey) -> Option<Arc<FrameworkScriptCandidates>> {
        self.entries.get(key).map(|e| Arc::clone(e.value()))
    }

    /// Memoize `candidates` under `key`, returning the canonical `Arc`.
    pub fn insert(
        &self,
        key: CandidateSlotKey,
        candidates: FrameworkScriptCandidates,
    ) -> Arc<FrameworkScriptCandidates> {
        let arc = Arc::new(candidates);
        self.entries.insert(key, Arc::clone(&arc));
        arc
    }

    /// Number of cached entries (test-only).
    #[cfg(test)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store has no entries (test-only).
    #[cfg(test)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The resolved-fact store sub-key.
///
/// NO `lib_env_hash` / `type_env_hash` — the resolved facts do not depend on
/// lib or type data. The `consumed_capability_bits` column folds the ON/OFF
/// state of the bits the provider consumed, so a capability flip misses the
/// stale slot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedFactKey {
    /// The owner file canonical.
    pub canonical: Arc<str>,
    /// The producing provider's adapter id.
    pub provider_id: FrameworkAdapterId,
    /// The producing provider's version.
    pub provider_version: u32,
    /// A stable hash over the ON/OFF state of the provider's consumed
    /// capability bits — a capability flip changes it.
    pub consumed_capability_bits: Hash16,
    /// The project identity (env isolation).
    pub project_identity: Hash16,
    /// The resolve-env dimension.
    pub resolve_env_hash: Hash16,
}

/// A cached resolved fact plus the validation rails the resolution observed.
#[derive(Clone)]
pub struct StoredResolvedFact {
    /// The fully-owned, immutable resolved payload.
    pub payload: Arc<dyn FrameworkScriptFactPayload>,
    /// Path-precise fact signature observed while resolving (covers the
    /// owner's content + every resolved import contributor).
    pub read_set_signature: ReadSetSignature,
    /// Project generation the entry was validated at.
    pub validated_at_generation: u64,
}

/// The resolved-fact store.
///
/// Query-identity discipline: warm reads pass a strict same-generation gate AND
/// a fact-rail validation against the caller's live view; cold writes admit
/// ONLY via [`SignatureAdmission::Cacheable`]. An overflowed signature is never
/// warmed.
#[derive(Default)]
pub struct FrameworkScriptFactStore {
    entries: DashMap<ResolvedFactKey, Arc<StoredResolvedFact>>,
}

impl std::fmt::Debug for FrameworkScriptFactStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameworkScriptFactStore")
            .field("entries_len", &self.entries.len())
            .finish()
    }
}

impl FrameworkScriptFactStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The cached resolved fact for `key` IFF it still validates under the live
    /// `view` and project `generation` — BOTH gates must pass.
    #[must_use]
    pub fn get_with_view<V: StoreView + ?Sized>(
        &self,
        key: &ResolvedFactKey,
        view: &V,
        generation: u64,
    ) -> Option<Arc<StoredResolvedFact>> {
        let candidate = Arc::clone(self.entries.get(key)?.value());
        if candidate.validated_at_generation != generation {
            return None;
        }
        if !view.validates_fact_signature(&candidate.read_set_signature.facts) {
            return None;
        }
        Some(candidate)
    }

    /// Admit `entry` under `key` ONLY when `admission` is
    /// [`SignatureAdmission::Cacheable`], returning the canonical `Arc` on
    /// admission.
    ///
    /// An overflowed / non-cacheable admission NEVER warms the store — the
    /// computed value is returned to the caller alone (the no-poison invariant).
    /// The returned `Arc` lets a non-admitting caller still hand back the
    /// computed payload without a store entry.
    pub(crate) fn publish_if_cacheable(
        &self,
        key: ResolvedFactKey,
        payload: Arc<dyn FrameworkScriptFactPayload>,
        admission: &SignatureAdmission,
        generation: u64,
    ) -> Arc<StoredResolvedFact> {
        let stored = Arc::new(StoredResolvedFact {
            payload,
            read_set_signature: admission
                .cacheable()
                .cloned()
                .unwrap_or_else(ReadSetSignature::empty),
            validated_at_generation: generation,
        });
        if admission.cacheable().is_some() {
            self.entries.insert(key, Arc::clone(&stored));
        }
        stored
    }

    /// The single cached entry, if exactly one exists (test-only — the
    /// fact-rail discriminating tests read the stored read-set signature).
    #[cfg(test)]
    #[must_use]
    pub fn only_entry(&self) -> Option<Arc<StoredResolvedFact>> {
        if self.entries.len() != 1 {
            return None;
        }
        self.entries.iter().next().map(|e| Arc::clone(e.value()))
    }

    /// Number of cached entries (test-only).
    #[cfg(test)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store has no entries (test-only).
    #[cfg(test)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The host-owned caches the resolved-validation half writes through.
///
/// Owned by the host; the registry owns the active-provider index instead.
#[derive(Debug, Default)]
pub struct FrameworkScriptCaches {
    /// The content-addressed candidate store.
    pub candidates: FrameworkScriptCandidateStore,
    /// The resolved-fact store.
    pub facts: FrameworkScriptFactStore,
}

impl FrameworkScriptCaches {
    /// An empty cache pair.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Resolve the adapter's framework script facts of type `T` for `canonical`.
///
/// Drives the resolved-validation half on demand:
/// 1. The registry's [`ActiveProviderIndex`](crate::framework::registry::ActiveProviderIndex)
///    selects the providers active for the file. An EMPTY index short-circuits
///    to `None` with zero per-file work (the steady state — no production
///    provider registers).
/// 2. For each active provider, the file's candidates are captured (content-
///    addressed slot peek/fill) by re-running the provider's syntax capture
///    over the file's live OXC program.
/// 3. The provider's resolved-validation (`validate`) is driven with the
///    resolved import targets (the session's own import resolution) + a
///    capability lookup; it rejects userland look-alikes and refuses on a
///    capability-bit miss.
/// 4. A resolved payload is admitted to the resolved-fact store ONLY when its
///    fact signature is [`SignatureAdmission::Cacheable`]; the cold compute
///    observes the owner's whole-hash + every resolved import contributor onto
///    the tracer so a cross-file edit misses the warm entry.
///
/// Returns `None` honestly when no active provider produces a `T` payload.
#[must_use]
pub(crate) fn resolve_script_facts<T: FrameworkScriptFactPayload>(
    host: &VerterHost,
    registration: &FrameworkRegistration,
    canonical: &str,
) -> Option<Arc<T>> {
    // Zero-cost fast path: this registration registers NO provider ⇒ no
    // candidates ⇒ no facts, so the resolved-validation half does ZERO per-file
    // work — no `current_eval_state`, no classification, no `get_analysis`, no
    // import resolution. In this program EVERY registration's provider list is
    // empty (no production provider registers), so the host registry's
    // `ActiveProviderIndex` is empty too — the two gates coincide in the
    // steady state. (The host index keys the registry-wide is_empty oracle; the
    // per-registration list keys this registration's own facts.)
    debug_assert!(
        host.framework_registry().active_provider_index().is_empty()
            == host
                .framework_registry()
                .get(&registration.descriptor.id)
                .is_none_or(|r| r.script_fact_providers.is_empty()),
        "the host index emptiness must agree with the registered adapter's provider list"
    );
    if registration.script_fact_providers.is_empty() {
        return None;
    }
    let (source, _framework_parse, whole_hash) = host.current_eval_state(canonical)?;
    let file_language = host.language_classifier().classify(canonical);
    let carrier_language = file_language.carrier_language_id();

    // The file's imports (specifier + the session-resolved canonical) — the
    // resolved data the provider's validation inspects. The session resolves
    // each candidate specifier through its OWN import resolver
    // (`resolve_snapshot_imports`) and hands the outcome to the provider as
    // data — the provider never reaches the resolver itself.
    let mut snapshot = host.get_analysis(canonical)?;
    host.resolve_snapshot_imports(canonical, &mut snapshot);
    let resolved_import_targets: Vec<ResolvedImportTarget> = snapshot
        .imports
        .iter()
        .map(|imp| ResolvedImportTarget {
            specifier: imp.source.clone(),
            resolved_canonical: imp.resolved_canonical_id.clone(),
        })
        .collect();

    // Select the active provider through the SHARED gate-matching authority
    // (`ActiveProviderIndex::gate_matches`) over THIS registration's own
    // providers — a registration produces only its own adapter's facts. The
    // shared predicate is exactly the one the registry-wide index applies, so
    // the per-registration selection and the index agree by construction.
    let provider = registration
        .script_fact_providers
        .iter()
        .find(|p| {
            crate::framework::registry::ActiveProviderIndex::gate_matches(
                &p.syntax_gate(),
                carrier_language,
                resolved_import_targets.iter().map(|t| t.specifier.as_str()),
            )
        })
        .cloned()?;

    // ── Candidate capture (content-addressed slot) ──
    let env = host.host_view_env_hashes_for(canonical);
    let candidate_canonical: Arc<str> = Arc::from(canonical);
    let candidate_key = CandidateSlotKey {
        canonical: Arc::clone(&candidate_canonical),
        content_hash: whole_hash,
        parse_env_hash: env.parse_env_hash,
        parser_version: crate::file_artifact_store::LEGACY_PARSER_VERSION,
        file_language_id: file_language.clone(),
        provider_id: provider.adapter_id(),
        provider_version: provider.provider_version(),
    };
    let candidates = host
        .framework_script_caches()
        .candidates
        .get(&candidate_key)
        .or_else(|| {
            capture_candidates_for(&provider, &source, &file_language).map(|c| {
                host.framework_script_caches()
                    .candidates
                    .insert(candidate_key.clone(), c)
            })
        })?;

    // ── Resolved-fact lookup ──
    let project_identity = host.host_view_project_identity_for(canonical).0;
    let consumed_capability_bits = capability_bits_hash(host, provider.consumed_capabilities());
    let fact_key = ResolvedFactKey {
        canonical: Arc::clone(&candidate_key.canonical),
        provider_id: provider.adapter_id(),
        provider_version: provider.provider_version(),
        consumed_capability_bits,
        project_identity,
        resolve_env_hash: env.resolve_env_hash,
    };
    let generation = host.project_type_store().project_generation();

    if let Some(current_view) = crate::typeinfo::current_store_view_for_query(host) {
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx =
            crate::resolver_core::HostResolverContext::from_current(host, &current_view, overlay);
        if let Some(stored) = host.framework_script_caches().facts.get_with_view(
            &fact_key,
            host_ctx.store_view(),
            generation,
        ) {
            return verter_semantic::analysis::framework_facts::downcast_fact_payload::<T>(
                Arc::clone(&stored.payload),
            );
        }
    }

    // The validation depends on the resolved import targets — i.e. the owner's
    // IMPORT ROUTE surface. A route change (a barrel / path-alias re-route that
    // points a specifier at a different canonical) leaves the owner's content
    // AND the old target's content unchanged, so the whole-hash rail alone
    // would stale-serve. The owner `ImportRoute` derived fact roots the cached
    // payload against that route surface.
    let owner_has_imports = !resolved_import_targets.is_empty();
    let import_route_hash = host.current_derived_fact_hash(
        canonical,
        crate::resolver_core::DerivedFactKind::ImportRoute,
    );

    // ── Cold resolved-validation, fact-traced ──
    let (payload_opt, finalise) = crate::fact_signature_helpers::install_fact_tracer(host, || {
        // Observe the owner's whole hash + every resolved import contributor so
        // a content edit to any of them misses the warm entry.
        crate::resolver_core::resolver_context::observe_fan_out(
            crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: canonical.to_string(),
                hash: whole_hash,
            },
        );
        // Root the payload against the owner's import-route surface so a
        // re-route (unchanged file contents) misses the warm entry.
        if let Some(hash) = import_route_hash {
            crate::resolver_core::resolver_context::observe_fan_out(
                crate::resolver_core::FactVersionRef::DerivedFactHash {
                    canonical_id: canonical.to_string(),
                    kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                    hash,
                },
            );
        }
        for target in &resolved_import_targets {
            if let Some(import_canonical) = &target.resolved_canonical {
                if let Some(h) = host.current_or_read_whole_hash(import_canonical) {
                    crate::resolver_core::resolver_context::observe_fan_out(
                        crate::resolver_core::FactVersionRef::FileWholeHash {
                            canonical_id: import_canonical.clone(),
                            hash: h,
                        },
                    );
                }
            }
        }
        let capability_on = |cap: &str| {
            host.language_classifier()
                .capability_is_enabled(&verter_language::CapabilityId::new(cap))
        };
        let cx = ResolvedValidationCx {
            candidates: &candidates,
            resolved_import_targets: &resolved_import_targets,
            capability_on: &capability_on,
        };
        provider.validate(cx)
    });

    let payload = payload_opt?;
    let admission = SignatureAdmission::from_finalise(finalise);
    // An import-dependent validation whose owner import-route rail could NOT be
    // produced must NOT warm the store (it would stale-serve on a re-route).
    // Return the computed value to this caller alone, uncached.
    if owner_has_imports && import_route_hash.is_none() {
        return verter_semantic::analysis::framework_facts::downcast_fact_payload::<T>(payload);
    }
    // Cacheable-only publication; overflow returns the value to this caller
    // alone (never warms the store).
    let stored = host.framework_script_caches().facts.publish_if_cacheable(
        fact_key,
        Arc::clone(&payload),
        &admission,
        generation,
    );
    verter_semantic::analysis::framework_facts::downcast_fact_payload::<T>(Arc::clone(
        &stored.payload,
    ))
}

/// Capture a provider's candidates from a file's source by re-running its
/// syntax-only capture over a freshly-parsed OXC program. PARSE-DOMAIN only.
fn capture_candidates_for(
    provider: &Arc<dyn ScriptFactProvider>,
    source: &str,
    file_language: &verter_language::FileLanguage,
) -> Option<FrameworkScriptCandidates> {
    let source_type = source_type_for_language(file_language);
    let alloc = Allocator::new();
    let parser = Parser::new(&alloc, source, source_type).with_options(ParseOptions {
        parse_regular_expression: false,
        ..ParseOptions::default()
    });
    let result = parser.parse();
    if result.panicked {
        return None;
    }
    let set = capture_script_candidates(std::slice::from_ref(provider), source, &result.program);
    set.per_provider.into_iter().next()
}

fn source_type_for_language(file_language: &verter_language::FileLanguage) -> SourceType {
    // Framework carrier files expose a TS script block; plain script files use
    // their own source type. Default to TS — the fixture seam parses TS.
    let _ = file_language;
    SourceType::ts()
}

fn capability_bits_hash(host: &VerterHost, consumed: &[&'static str]) -> Hash16 {
    let mut buf = Vec::new();
    for cap in consumed {
        let on = host
            .language_classifier()
            .capability_is_enabled(&verter_language::CapabilityId::new(cap));
        buf.extend_from_slice(cap.as_bytes());
        buf.push(if on { 1 } else { 0 });
        buf.push(0);
    }
    crate::hash::hash_16(&buf)
}

/// Shared in-tree fixture seam — a `ScriptFactProvider` and registration
/// builders exercising the resolved-validation half end-to-end. Test-only: no
/// production provider registers (Vue's macro analysis stays in the shallow
/// pass), so the fixture is the sole exerciser of capture → active-set
/// selection → resolved-validation → content-addressed cache.
#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use std::any::Any;
    use verter_language::LanguageId;
    use verter_semantic::analysis::framework_facts::{ScriptCandidateCx, ScriptFactSyntaxGate};

    /// The fixture adapter id.
    pub(crate) fn fixture_adapter_id() -> FrameworkAdapterId {
        FrameworkAdapterId::new("fixture-fw")
    }

    /// The fixture carrier language id (for the carrier-gated registration).
    pub(crate) fn fixture_language() -> LanguageId {
        LanguageId::new("fixture-lang")
    }

    /// The fixture import specifier (for the import-gated registration). The
    /// end-to-end host fixture imports this exact specifier, and the host's
    /// import resolver resolves it to the framework package file.
    pub(crate) const FIXTURE_IMPORT_SPECIFIER: &str = "./node_modules/fixture-fw/index";

    /// The capability bit the fixture provider's resolved facts depend on.
    pub(crate) const FIXTURE_CAPABILITY: &str = "fixture-cap";

    /// The fixture resolved payload.
    #[derive(Debug)]
    pub(crate) struct FixtureFactPayload {
        pub(crate) resolved_specifier: String,
    }
    impl FrameworkScriptFactPayload for FixtureFactPayload {
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
            self
        }
    }

    /// The fixture provider. Captures a candidate whenever invoked; validates
    /// that a resolved import landed inside the framework's package directory
    /// and (when `requires_capability`) the capability bit is ON.
    ///
    /// `requires_capability = false` lets an end-to-end test exercise the full
    /// resolve flow on a real host (whose capability snapshot is empty in this
    /// program) without depending on a capability producer that does not exist
    /// yet; the capability-OFF refusal is then a separate discriminating case.
    pub(crate) struct FixtureProvider {
        pub(crate) gate: ScriptFactSyntaxGate,
        pub(crate) requires_capability: bool,
    }

    impl ScriptFactProvider for FixtureProvider {
        fn adapter_id(&self) -> FrameworkAdapterId {
            fixture_adapter_id()
        }
        fn provider_version(&self) -> u32 {
            1
        }
        fn syntax_gate(&self) -> ScriptFactSyntaxGate {
            self.gate.clone()
        }
        fn consumed_capabilities(&self) -> &[&'static str] {
            if self.requires_capability {
                &[FIXTURE_CAPABILITY]
            } else {
                &[]
            }
        }
        fn capture(&self, _cx: ScriptCandidateCx<'_>) -> Option<FrameworkScriptCandidates> {
            Some(FrameworkScriptCandidates {
                adapter_id: self.adapter_id(),
                provider_version: self.provider_version(),
                stable_hash: [7u8; 16],
                payload: Arc::new(()),
            })
        }
        fn validate(
            &self,
            cx: ResolvedValidationCx<'_>,
        ) -> Option<Arc<dyn FrameworkScriptFactPayload>> {
            if self.requires_capability && !(cx.capability_on)(FIXTURE_CAPABILITY) {
                return None;
            }
            let resolved = cx.resolved_import_targets.iter().find(|t| {
                t.resolved_canonical
                    .as_deref()
                    .is_some_and(|c| c.contains("/node_modules/fixture-fw/"))
            })?;
            Some(Arc::new(FixtureFactPayload {
                resolved_specifier: resolved.specifier.clone(),
            }))
        }
    }

    fn fixture_store() -> Arc<dyn crate::framework::surface_store::ErasedFrameworkSurfaceStore> {
        Arc::new(crate::framework::surface_store::FrameworkSurfaceStore::<
            crate::typeinfo::framework_surface::VueSurfaceKey,
            crate::typeinfo::framework_surface::MacroSurfaceDtos,
        >::new())
    }

    fn registration_with(
        gate: ScriptFactSyntaxGate,
        requires_capability: bool,
    ) -> FrameworkRegistration {
        FrameworkRegistration {
            descriptor: crate::framework::descriptor::FrameworkAdapterDescriptor {
                id: fixture_adapter_id(),
                tag: verter_protocol::typeinfo::graph::FrameworkTag::Svelte,
                supported_surfaces: crate::framework::descriptor::ALL_FRAMEWORK_SURFACE_KINDS,
                carrier_language: None,
                virtual_file_naming: None,
                supports_named_export_surfaces: false,
            },
            carrier: None,
            synth: None,
            api_projector: None,
            script_fact_providers: vec![Arc::new(FixtureProvider {
                gate,
                requires_capability,
            })],
            surface: crate::framework::registry::SurfaceRegistration::Deferred,
            surface_store: fixture_store(),
        }
    }

    /// A fixture registration whose provider is carrier-language-gated.
    pub(crate) fn carrier_gated_fixture_registration() -> FrameworkRegistration {
        registration_with(
            ScriptFactSyntaxGate::CarrierLanguage(fixture_language()),
            true,
        )
    }

    /// A fixture registration whose provider is import-specifier-gated and
    /// requires a capability bit (the capability-gated path).
    pub(crate) fn import_gated_fixture_registration() -> FrameworkRegistration {
        registration_with(
            ScriptFactSyntaxGate::ImportSpecifier(FIXTURE_IMPORT_SPECIFIER),
            true,
        )
    }

    /// A fixture registration whose provider is import-specifier-gated and
    /// requires NO capability — the variant an end-to-end test exercises on a
    /// real host (whose capability snapshot is empty in this program).
    pub(crate) fn import_gated_capability_free_fixture_registration() -> FrameworkRegistration {
        registration_with(
            ScriptFactSyntaxGate::ImportSpecifier(FIXTURE_IMPORT_SPECIFIER),
            false,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache_runtime::SignatureAdmission;
    use crate::resolver_core::{
        FactVersionRef, PermissiveStoreView, StoreView, StoreViewCompatToken,
    };
    use verter_semantic::analysis::framework_facts::FrameworkScriptCandidates;

    fn candidate_key(canonical: &str, content: [u8; 16]) -> CandidateSlotKey {
        CandidateSlotKey {
            canonical: Arc::from(canonical),
            content_hash: content,
            parse_env_hash: [0u8; 16],
            parser_version: crate::file_artifact_store::LEGACY_PARSER_VERSION,
            file_language_id: crate::file_artifact_store::FileArtifactKey::derived_file_language_id(
                canonical,
            ),
            provider_id: FrameworkAdapterId::new("fixture-fw"),
            provider_version: 1,
        }
    }

    fn fixture_candidates() -> FrameworkScriptCandidates {
        FrameworkScriptCandidates {
            adapter_id: FrameworkAdapterId::new("fixture-fw"),
            provider_version: 1,
            stable_hash: [1u8; 16],
            payload: Arc::new(()),
        }
    }

    #[test]
    fn candidate_store_is_content_addressed_hit_and_version_miss() {
        let store = FrameworkScriptCandidateStore::new();
        let key = candidate_key("/a.ts", [3u8; 16]);
        store.insert(key.clone(), fixture_candidates());
        // Same key ⇒ hit.
        assert!(store.get(&key).is_some());
        // A content edit (different content_hash) ⇒ a DIFFERENT key ⇒ miss.
        let edited = candidate_key("/a.ts", [4u8; 16]);
        assert!(store.get(&edited).is_none());
        // A provider upgrade (different provider_version) ⇒ miss.
        let upgraded = CandidateSlotKey {
            provider_version: 2,
            ..key.clone()
        };
        assert!(store.get(&upgraded).is_none());
    }

    fn fixture_payload() -> Arc<dyn FrameworkScriptFactPayload> {
        Arc::new(fixtures::FixtureFactPayload {
            resolved_specifier: "@corp/fixture-fw".to_string(),
        })
    }

    fn resolved_fact_key(canonical: &str) -> ResolvedFactKey {
        ResolvedFactKey {
            canonical: Arc::from(canonical),
            provider_id: FrameworkAdapterId::new("fixture-fw"),
            provider_version: 1,
            consumed_capability_bits: [0u8; 16],
            project_identity: [0u8; 16],
            resolve_env_hash: [0u8; 16],
        }
    }

    /// A view that REJECTS every fact — discriminates the fact-rail gate.
    struct RejectingView;
    impl StoreView for RejectingView {
        fn compat_token(&self) -> StoreViewCompatToken {
            StoreViewCompatToken {
                epoch: 0,
                session: None,
                validity_fingerprint: 0,
            }
        }
        fn validates(&self, _fact: &FactVersionRef) -> bool {
            false
        }
    }

    #[test]
    fn resolved_fact_warm_read_requires_same_generation_and_fact_rail() {
        let store = FrameworkScriptFactStore::new();
        let key = resolved_fact_key("/a.ts");
        // Admit a Cacheable entry carrying a tracked cross-file fact at gen 5.
        let cross_file = FactVersionRef::FileWholeHash {
            canonical_id: "/node_modules/fixture-fw/index.d.ts".to_string(),
            hash: [9u8; 16],
        };
        let admission = SignatureAdmission::Cacheable(ReadSetSignature::new(Arc::from(
            vec![cross_file].into_boxed_slice(),
        )));
        store.publish_if_cacheable(key.clone(), fixture_payload(), &admission, 5);

        // Same gen + permissive view ⇒ warm hit.
        assert!(store.get_with_view(&key, &PermissiveStoreView, 5).is_some());
        // Generation bump ⇒ miss (strict same-generation gate).
        assert!(store.get_with_view(&key, &PermissiveStoreView, 6).is_none());
        // Right gen but a view that rejects the tracked fact ⇒ miss (fact rail).
        assert!(store.get_with_view(&key, &RejectingView, 5).is_none());
    }

    #[test]
    fn overflowed_admission_never_warms_the_store_return_only() {
        let store = FrameworkScriptFactStore::new();
        let key = resolved_fact_key("/a.ts");
        // An overflowed (NonCacheable) admission: the value is returned to the
        // caller but the store is NOT warmed (the no-poison invariant).
        let admission =
            SignatureAdmission::from_finalise(crate::resolver_core::FactReadSetFinalise::Overflow);
        let stored = store.publish_if_cacheable(key.clone(), fixture_payload(), &admission, 5);
        // The computed value is handed back...
        assert!(
            verter_semantic::analysis::framework_facts::downcast_fact_payload::<
                fixtures::FixtureFactPayload,
            >(Arc::clone(&stored.payload))
            .is_some()
        );
        // ...but the store stays empty — the overflowed result was NOT warmed.
        assert!(store.is_empty());
        assert!(store.get_with_view(&key, &PermissiveStoreView, 5).is_none());
    }

    #[test]
    fn cacheable_admission_warms_exactly_one_entry() {
        let store = FrameworkScriptFactStore::new();
        let key = resolved_fact_key("/a.ts");
        let admission = SignatureAdmission::Cacheable(ReadSetSignature::empty());
        store.publish_if_cacheable(key, fixture_payload(), &admission, 5);
        assert_eq!(store.len(), 1, "a Cacheable admission warms one entry");
    }

    use crate::{HostConfig, UpsertRequest, VerterHost};
    use verter_language::FileLanguage;

    fn host_with_files() -> std::sync::Arc<VerterHost> {
        let host = std::sync::Arc::new(VerterHost::new_standalone(HostConfig::default()));
        // The framework package file (its canonical contains the package dir
        // the fixture provider's resolved-validation checks for).
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/proj/node_modules/fixture-fw/index.ts".to_string(),
                source: Arc::from("export const marker = 1;"),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert package file");
        // The consumer file importing the package by the GATED specifier.
        let _ = host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/proj/Consumer.ts".to_string(),
            source: Arc::from(
                "import { marker } from './node_modules/fixture-fw/index';\nexport const x = marker;",
            ),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert consumer file");
        host
    }

    #[test]
    fn fixture_provider_resolves_validates_and_caches_end_to_end() {
        let host = host_with_files();
        let registration = fixtures::import_gated_capability_free_fixture_registration();
        // First resolve: cold compute through capture → active-set selection →
        // resolved-validation → content-addressed cache.
        let facts = resolve_script_facts::<fixtures::FixtureFactPayload>(
            &host,
            &registration,
            "/proj/Consumer.ts",
        )
        .expect("the consumer imports the framework package, so facts resolve");
        assert_eq!(facts.resolved_specifier, "./node_modules/fixture-fw/index");
        // The content-addressed candidate slot was filled.
        assert!(
            !host.framework_script_caches().candidates.is_empty(),
            "the candidate slot is filled on the cold capture"
        );
        // The resolved-fact slot warmed (a Cacheable admission).
        assert!(
            !host.framework_script_caches().facts.is_empty(),
            "the resolved fact warmed the store (Cacheable admission)"
        );
        // The cached entry's read-set ROOTS the payload against the owner's
        // IMPORT-ROUTE surface (not just whole-hashes) — a re-route that leaves
        // file contents unchanged must still invalidate. (If the ImportRoute
        // fact were dropped, this assertion fails.)
        let stored = host
            .framework_script_caches()
            .facts
            .only_entry()
            .expect("exactly one resolved fact is cached");
        let has_import_route_fact = stored.read_set_signature.facts.iter().any(|f| {
            matches!(
                f,
                crate::resolver_core::FactVersionRef::DerivedFactHash {
                    canonical_id,
                    kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                    ..
                } if canonical_id == "/proj/Consumer.ts"
            )
        });
        assert!(
            has_import_route_fact,
            "the cached resolved fact must observe the owner's ImportRoute \
             derived fact so a re-route invalidates it"
        );
        // Second resolve: warm hit returns the same typed payload.
        let facts2 = resolve_script_facts::<fixtures::FixtureFactPayload>(
            &host,
            &registration,
            "/proj/Consumer.ts",
        )
        .expect("warm hit");
        assert_eq!(facts2.resolved_specifier, facts.resolved_specifier);
    }

    #[test]
    fn capability_off_refuses_through_the_real_host() {
        // The real host's capability snapshot is empty, so the
        // capability-REQUIRING fixture provider refuses — even though the
        // import resolves to the framework package.
        let host = host_with_files();
        let registration = fixtures::import_gated_fixture_registration();
        let facts = resolve_script_facts::<fixtures::FixtureFactPayload>(
            &host,
            &registration,
            "/proj/Consumer.ts",
        );
        assert!(
            facts.is_none(),
            "the consumed capability bit is OFF on the real host, so the \
             provider refuses to emit resolved facts"
        );
        // The refusal does NOT warm the resolved-fact store.
        assert!(host.framework_script_caches().facts.is_empty());
    }

    #[test]
    fn no_provider_registration_is_zero_cost_none() {
        // A registration with no provider answers None without touching the
        // host's parse/analysis at all (the steady-state zero-cost path).
        let host = host_with_files();
        let registration = fixtures::import_gated_capability_free_fixture_registration();
        let empty_registration = FrameworkRegistration {
            script_fact_providers: Vec::new(),
            ..registration
        };
        let facts = resolve_script_facts::<fixtures::FixtureFactPayload>(
            &host,
            &empty_registration,
            "/proj/Consumer.ts",
        );
        assert!(facts.is_none());
        assert!(host.framework_script_caches().candidates.is_empty());
    }

    #[test]
    fn gate_miss_file_does_not_import_specifier_resolves_none() {
        // A file that does NOT import the gated specifier selects no provider
        // (the gate misses), so no facts resolve.
        let host = std::sync::Arc::new(VerterHost::new_standalone(HostConfig::default()));
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/proj/Unrelated.ts".to_string(),
                source: Arc::from("export const y = 1;"),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert unrelated file");
        let registration = fixtures::import_gated_capability_free_fixture_registration();
        let facts = resolve_script_facts::<fixtures::FixtureFactPayload>(
            &host,
            &registration,
            "/proj/Unrelated.ts",
        );
        assert!(facts.is_none(), "the gate misses — no provider is active");
    }
}
